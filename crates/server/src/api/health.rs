//! Liveness and readiness endpoints.

use axum::{Json, extract::State};
use serde::Serialize;
use utoipa::ToSchema;

use crate::{ApiResult, AppState};

/// Health check response.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
pub struct Health {
    /// Always `"ok"` when healthy.
    pub status: &'static str,
}

/// Liveness probe: the process is serving.
#[utoipa::path(get, path = "/healthz", tag = "health", responses((status = 200, body = Health)))]
pub async fn healthz() -> Json<Health> {
    Json(Health { status: "ok" })
}

/// Readiness probe: the database answers.
#[utoipa::path(
    get,
    path = "/readyz",
    tag = "health",
    responses((status = 200, body = Health), (status = 500, body = crate::error::ErrorBody))
)]
pub async fn readyz(State(state): State<AppState>) -> ApiResult<Json<Health>> {
    let _ = sqlx::query("SELECT 1").execute(&state.db).await?;
    Ok(Json(Health { status: "ok" }))
}
