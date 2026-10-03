//! Router assembly and shared application state.

use std::sync::Arc;

use axum::{Json, Router, http::HeaderName, routing::get};
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{api, config::Config, error::ErrorBody};

/// State shared by every handler. Cheap to clone.
#[derive(Debug, Clone)]
pub struct AppState {
    /// Runtime configuration.
    pub config: Arc<Config>,
}

impl AppState {
    /// Builds state from configuration.
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Courtpit API",
        version = "0.1.0",
        description = "Courtpit REST API."
    ),
    components(schemas(ErrorBody))
)]
struct ApiDoc;

/// All API routes plus the OpenAPI document describing them.
pub fn api_router() -> (Router<AppState>, utoipa::openapi::OpenApi) {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(api::health::healthz))
        .split_for_parts()
}

/// The complete application router, ready to serve.
pub fn router(state: AppState) -> Router {
    let (api, openapi) = api_router();
    let openapi = Arc::new(openapi);
    let request_id = HeaderName::from_static("x-request-id");
    api.route(
        "/api/v1/openapi.json",
        get(move || {
            let doc = Arc::clone(&openapi);
            async move { Json(doc.as_ref().clone()) }
        }),
    )
    .layer(PropagateRequestIdLayer::new(request_id.clone()))
    .layer(TraceLayer::new_for_http())
    .layer(SetRequestIdLayer::new(request_id, MakeRequestUuid))
    .with_state(state)
}
