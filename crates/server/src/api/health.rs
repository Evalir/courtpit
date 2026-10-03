//! Liveness endpoint.

use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

/// Health check response.
#[derive(Debug, Serialize, ToSchema)]
pub struct Health {
    /// Always `"ok"` when the process is serving.
    pub status: &'static str,
}

/// Liveness probe.
#[utoipa::path(get, path = "/healthz", tag = "health", responses((status = 200, body = Health)))]
pub async fn healthz() -> Json<Health> {
    Json(Health { status: "ok" })
}
