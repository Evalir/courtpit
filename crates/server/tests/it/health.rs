use axum::http::{Method, StatusCode};

use crate::common::TestApp;

#[tokio::test]
async fn healthz_ok() {
    let app = TestApp::new();
    let (status, body) = app.send(Method::GET, "/healthz").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn openapi_lists_healthz() {
    let app = TestApp::new();
    let (status, body) = app.send(Method::GET, "/api/v1/openapi.json").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["paths"]["/healthz"]["get"].is_object());
}

#[tokio::test]
async fn unknown_route_is_404() {
    let app = TestApp::new();
    let (status, _) = app.send(Method::GET, "/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
