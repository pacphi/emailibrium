//! SPIKE (throwaway): can SeaORM replace the hand-rolled Database enum + adapt() +
//! per-backend match dispatch — one code path for both SQLite and Postgres?
//!
//! Ports the hardest operations from cleanup/repository/plan_repo.rs and runs them
//! against BOTH backends, wrapping the pools our existing Database::connect() creates
//! (proving incremental adoption can share one pool with legacy sqlx code).
//!
//! Spike checklist (from ADR-035's divergence catalog):
//!   Q1  single sqlx version in tree                        (checked via cargo tree)
//!   Q2  wrap existing sqlx pools                            (composition root below)
//!   Q3  Vec<u8> (16-byte UUID) PKs incl. composite          (plans.id, ops.(plan_id,seq))
//!   Q4  one-code-path upsert (ON CONFLICT)                  (save_plan)
//!   Q5  one-code-path transactions                          (save_plan, replace_ops)
//!   Q6  dynamic/optional filters + cursor pagination        (list_ops)
//!   Q7  aggregate MAX(seq) decode                           (max_seq)
//!   Q8  json_set elimination via read-modify-write          (update_op_status)
//!   Q9  update_many + rows_affected                         (cancel_plan)
//!   Q10 per-backend raw-SQL escape hatch (AVG cast, FTS)    (avg_seq_raw)
//!   Q11 repo fns generic over ConnectionTrait (conn OR txn) (insert_op)

use emailibrium::db::Database;
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, FromQueryResult, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Statement,
    TransactionTrait,
};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Entities — partial models over the tables migrations 024 already creates.
// The entity IS the single source of truth for Rust-side types: seq is i32
// (INT4), ids are Vec<u8> (BLOB/BYTEA), created_at is i64 (BIGINT) — the
// decode-width bug class from ADR-035 becomes unrepresentable.
// ---------------------------------------------------------------------------

mod plans {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "cleanup_plans")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Vec<u8>,
        pub user_id: Vec<u8>,
        pub created_at: i64,
        pub valid_until: i64,
        pub plan_hash: Vec<u8>,
        pub status: String,
        pub totals_json: String,
        pub risk_json: String,
        pub warnings_json: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

mod ops {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "cleanup_plan_operations")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub plan_id: Vec<u8>,
        #[sea_orm(primary_key, auto_increment = false)]
        pub seq: i32,
        pub op_kind: String,
        pub account_id: Vec<u8>,
        pub email_id: Option<Vec<u8>>,
        pub action: String,
        pub source_kind: String,
        pub risk: String,
        pub status: String,
        pub payload_json: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// ---------------------------------------------------------------------------
// Q11: repo helpers generic over ConnectionTrait — the same fn works with a
// live connection OR an open transaction, on either backend. This is the
// shape repository bodies would take after adoption.
// ---------------------------------------------------------------------------

async fn insert_op<C: ConnectionTrait>(
    c: &C,
    plan_id: &[u8],
    seq: i32,
    account: &str,
    status: &str,
) -> Result<(), sea_orm::DbErr> {
    ops::ActiveModel {
        plan_id: Set(plan_id.to_vec()),
        seq: Set(seq),
        op_kind: Set("materialized".into()),
        account_id: Set(account.as_bytes().to_vec()),
        email_id: Set(Some(format!("e{seq}").into_bytes())),
        action: Set("\"archive\"".into()),
        source_kind: Set("manual".into()),
        risk: Set("low".into()),
        status: Set(status.into()),
        payload_json: Set(Some(format!("{{\"seq\":{seq},\"status\":\"{status}\"}}"))),
    }
    .insert(c)
    .await?;
    Ok(())
}

// Q4 + Q5: upsert parent + replace children, one transaction, one code path.
async fn save_plan(orm: &DatabaseConnection, plan_id: &[u8], user: &str, n_ops: i32) -> Result<(), sea_orm::DbErr> {
    let txn = orm.begin().await?;

    let am = plans::ActiveModel {
        id: Set(plan_id.to_vec()),
        user_id: Set(user.as_bytes().to_vec()),
        created_at: Set(1_700_000_000_000),
        valid_until: Set(1_700_000_000_000 + 1_800_000),
        plan_hash: Set(vec![0u8; 32]),
        status: Set("ready".into()),
        totals_json: Set("{}".into()),
        risk_json: Set("{}".into()),
        warnings_json: Set("[]".into()),
    };
    plans::Entity::insert(am)
        .on_conflict(
            OnConflict::column(plans::Column::Id)
                .update_columns([
                    plans::Column::UserId,
                    plans::Column::Status,
                    plans::Column::TotalsJson,
                ])
                .to_owned(),
        )
        .exec(&txn)
        .await?;

    ops::Entity::delete_many()
        .filter(ops::Column::PlanId.eq(plan_id.to_vec()))
        .exec(&txn)
        .await?;
    for seq in 1..=n_ops {
        insert_op(&txn, plan_id, seq, "acct-a", "pending").await?;
    }

    txn.commit().await?;
    Ok(())
}

// Q6: optional filters + seq-cursor pagination, one code path.
async fn list_ops(
    orm: &DatabaseConnection,
    plan_id: &[u8],
    account: Option<&str>,
    cursor: i32,
    limit: u64,
) -> Result<Vec<ops::Model>, sea_orm::DbErr> {
    let mut q = ops::Entity::find()
        .filter(ops::Column::PlanId.eq(plan_id.to_vec()))
        .filter(ops::Column::Seq.gt(cursor));
    if let Some(a) = account {
        q = q.filter(ops::Column::AccountId.eq(a.as_bytes().to_vec()));
    }
    q.order_by_asc(ops::Column::Seq).limit(limit).all(orm).await
}

// Q7: aggregate decode without hand-picking INT4-vs-dynamic widths.
async fn max_seq(orm: &DatabaseConnection, plan_id: &[u8]) -> Result<i32, sea_orm::DbErr> {
    #[derive(FromQueryResult)]
    struct MaxRow {
        max_seq: Option<i32>,
    }
    let row = ops::Entity::find()
        .select_only()
        .column_as(ops::Column::Seq.max(), "max_seq")
        .filter(ops::Column::PlanId.eq(plan_id.to_vec()))
        .into_model::<MaxRow>()
        .one(orm)
        .await?;
    Ok(row.and_then(|r| r.max_seq).unwrap_or(0))
}

// Q8: the json_set/jsonb_set divergence disappears — read payload, edit in
// Rust, write back, in one transaction. One code path, both backends.
async fn update_op_status(
    orm: &DatabaseConnection,
    plan_id: &[u8],
    seq: i32,
    new_status: &str,
) -> Result<(), sea_orm::DbErr> {
    let txn = orm.begin().await?;
    let row = ops::Entity::find_by_id((plan_id.to_vec(), seq))
        .one(&txn)
        .await?
        .ok_or_else(|| sea_orm::DbErr::Custom("op not found".into()))?;

    let payload = row
        .payload_json
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .map(|mut v| {
            v["status"] = serde_json::Value::String(new_status.to_string());
            v.to_string()
        });

    let mut am: ops::ActiveModel = row.into();
    am.status = Set(new_status.into());
    am.payload_json = Set(payload);
    am.update(&txn).await?;
    txn.commit().await?;
    Ok(())
}

// Q9: bulk update + rows_affected.
async fn cancel_plan(orm: &DatabaseConnection, plan_id: &[u8]) -> Result<u64, sea_orm::DbErr> {
    let res = plans::Entity::update_many()
        .col_expr(
            plans::Column::Status,
            sea_orm::sea_query::Expr::value("cancelled"),
        )
        .filter(plans::Column::Id.eq(plan_id.to_vec()))
        .exec(orm)
        .await?;
    Ok(res.rows_affected)
}

// Q10: the escape hatch for the irreducible cases (AVG-cast today, FTS in
// phase 5) — per-backend raw SQL, cleanly scoped, still one call site.
async fn avg_seq_raw(orm: &DatabaseConnection, plan_id: &[u8]) -> Result<f64, sea_orm::DbErr> {
    let backend = orm.get_database_backend();
    let sql = match backend {
        sea_orm::DatabaseBackend::Postgres => {
            "SELECT AVG(seq)::float8 AS a FROM cleanup_plan_operations WHERE plan_id = $1"
        }
        _ => "SELECT AVG(seq) AS a FROM cleanup_plan_operations WHERE plan_id = ?",
    };
    let stmt = Statement::from_sql_and_values(backend, sql, [plan_id.to_vec().into()]);
    let row = orm
        .query_one_raw(stmt)
        .await?
        .ok_or_else(|| sea_orm::DbErr::Custom("no row".into()))?;
    let avg: Option<f64> = row.try_get("", "a")?;
    Ok(avg.unwrap_or(0.0))
}

// ---------------------------------------------------------------------------
// The full scenario, identical for both backends.
// ---------------------------------------------------------------------------

async fn run_scenario(label: &str, orm: &DatabaseConnection) -> Result<(), Box<dyn std::error::Error>> {
    let plan_id = Uuid::now_v7().as_bytes().to_vec();

    // Q3/Q4/Q5: insert path, then upsert path over the same PK.
    save_plan(orm, &plan_id, "user-1", 3).await?;
    save_plan(orm, &plan_id, "user-1", 3).await?; // ON CONFLICT DO UPDATE
    println!("[{label}] save_plan insert+upsert ok (Vec<u8> PK, txn, ON CONFLICT)");

    // find_by_id on a Vec<u8> PK and on a composite (Vec<u8>, i32) PK.
    let loaded = plans::Entity::find_by_id(plan_id.clone()).one(orm).await?;
    assert_eq!(loaded.as_ref().map(|p| p.status.as_str()), Some("ready"));
    let op2 = ops::Entity::find_by_id((plan_id.clone(), 2)).one(orm).await?;
    assert!(op2.is_some());
    println!("[{label}] find_by_id simple + composite PK ok");

    // Q6: pagination + optional filter.
    let page = list_ops(orm, &plan_id, None, 0, 2).await?;
    assert_eq!(page.iter().map(|o| o.seq).collect::<Vec<_>>(), vec![1, 2]);
    let page2 = list_ops(orm, &plan_id, Some("acct-a"), 2, 10).await?;
    assert_eq!(page2.iter().map(|o| o.seq).collect::<Vec<_>>(), vec![3]);
    let none = list_ops(orm, &plan_id, Some("acct-zzz"), 0, 10).await?;
    assert!(none.is_empty());
    println!("[{label}] dynamic filters + cursor pagination ok");

    // Q7.
    assert_eq!(max_seq(orm, &plan_id).await?, 3);
    println!("[{label}] MAX(seq) -> i32 ok");

    // Q8.
    update_op_status(orm, &plan_id, 2, "applied").await?;
    let op2 = ops::Entity::find_by_id((plan_id.clone(), 2))
        .one(orm)
        .await?
        .unwrap();
    assert_eq!(op2.status, "applied");
    let payload: serde_json::Value = serde_json::from_str(op2.payload_json.as_deref().unwrap())?;
    assert_eq!(payload["status"], "applied");
    println!("[{label}] update_op_status read-modify-write ok (json_set eliminated)");

    // count() convenience while we're here.
    let n = ops::Entity::find()
        .filter(ops::Column::PlanId.eq(plan_id.clone()))
        .count(orm)
        .await?;
    assert_eq!(n, 3);

    // Q10.
    let avg = avg_seq_raw(orm, &plan_id).await?;
    assert!((avg - 2.0).abs() < f64::EPSILON, "avg was {avg}");
    println!("[{label}] raw escape hatch AVG ok ({avg})");

    // Q9.
    assert_eq!(cancel_plan(orm, &plan_id).await?, 1);
    let cancelled = plans::Entity::find_by_id(plan_id.clone()).one(orm).await?.unwrap();
    assert_eq!(cancelled.status, "cancelled");
    println!("[{label}] update_many + rows_affected ok");

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --- SQLite: our existing Database::connect + migrations, pool wrapped (Q2). ---
    let tmp = std::env::temp_dir().join(format!("spike_seaorm_{}.db", Uuid::new_v4()));
    let sqlite_url = format!("sqlite://{}?mode=rwc", tmp.display());
    let db = Database::connect(&sqlite_url).await?;
    db.run_migrations().await?;
    let orm_sqlite: DatabaseConnection = match &db {
        Database::Sqlite(pool) => {
            sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool.clone())
        }
        _ => unreachable!(),
    };
    run_scenario("sqlite", &orm_sqlite).await?;
    drop(orm_sqlite);
    let _ = std::fs::remove_file(&tmp);

    // --- Postgres: same scenario, same code path, live container (Q2). ---
    let pg_url = "postgres://postgres:test@localhost:55520/emailibrium_test";
    let db = Database::connect(pg_url).await?;
    db.run_migrations().await?;
    let orm_pg: DatabaseConnection = match &db {
        Database::Postgres(pool) => {
            sea_orm::SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone())
        }
        _ => unreachable!(),
    };
    run_scenario("postgres", &orm_pg).await?;

    println!("SPIKE_SEAORM_OK");
    Ok(())
}
