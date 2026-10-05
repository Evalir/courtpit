use axum::http::StatusCode;

use crate::common::TestApp;

#[tokio::test]
async fn healthz_ok() {
    let app = TestApp::spawn().await;
    let body = app.get("/healthz").send().await.expect(StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn readyz_checks_database() {
    let app = TestApp::spawn().await;
    let body = app.get("/readyz").send().await.expect(StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn openapi_lists_health_endpoints() {
    let app = TestApp::spawn().await;
    let body = app
        .get("/api/v1/openapi.json")
        .send()
        .await
        .expect(StatusCode::OK);
    assert!(body["paths"]["/healthz"]["get"].is_object());
    assert!(body["paths"]["/readyz"]["get"].is_object());
}

#[tokio::test]
async fn openapi_route_serves_the_generated_document() {
    let app = TestApp::spawn().await;
    let served = app
        .get("/api/v1/openapi.json")
        .send()
        .await
        .expect(StatusCode::OK);
    let generated = serde_json::to_value(racquetcollective_server::app::openapi()).unwrap();
    assert_eq!(served, generated);
}

#[tokio::test]
async fn unknown_route_is_404() {
    let app = TestApp::spawn().await;
    let res = app.get("/nope").send().await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
}
