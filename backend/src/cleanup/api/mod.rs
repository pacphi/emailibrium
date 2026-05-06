//! REST routes for cleanup planning (ADR-030 §9).

pub mod apply;
pub mod plan;

use axum::Router;

use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().merge(plan::routes()).merge(apply::routes())
}
