//! REST routes for cleanup planning (ADR-030 §9).

pub mod plan;

use axum::Router;

use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().merge(plan::routes())
}
