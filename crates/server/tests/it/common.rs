//! Shared test harness.

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

/// An in-process application under test.
pub struct TestApp {
    pub router: Router,
}

impl TestApp {
    /// Builds the app with default configuration.
    pub fn new() -> Self {
        let state = courtpit_server::AppState::new(courtpit_server::Config::default());
        Self {
            router: courtpit_server::router(state),
        }
    }

    /// Sends a request and returns status plus parsed JSON body (`Null` when empty).
    pub async fn send(&self, method: Method, uri: &str) -> (StatusCode, Value) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let json = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, json)
    }
}
