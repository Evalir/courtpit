use axum::http::{HeaderName, StatusCode, header};
use racquetcollective_server::Config;

use crate::common::{Res, TestApp, test_config};

/// Asserts the security headers every response carries (decision 104); HSTS only over HTTPS.
#[track_caller]
pub(crate) fn assert_security_headers(res: &Res, https: bool) {
    let get = |name: HeaderName| res.headers.get(name).map(|value| value.to_str().unwrap());
    assert_eq!(get(header::X_CONTENT_TYPE_OPTIONS), Some("nosniff"));
    assert_eq!(get(header::REFERRER_POLICY), Some("strict-origin-when-cross-origin"));
    assert_eq!(get(header::CONTENT_SECURITY_POLICY), Some("frame-ancestors 'none'"));
    assert_eq!(get(header::X_FRAME_OPTIONS), Some("DENY"));
    assert_eq!(
        get(header::STRICT_TRANSPORT_SECURITY),
        https.then_some("max-age=63072000; includeSubDomains")
    );
}

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
    let body = app.get("/api/v1/openapi.json").send().await.expect(StatusCode::OK);
    assert!(body["paths"]["/healthz"]["get"].is_object());
    assert!(body["paths"]["/readyz"]["get"].is_object());
}

#[tokio::test]
async fn openapi_route_serves_the_generated_document() {
    let app = TestApp::spawn().await;
    let served = app.get("/api/v1/openapi.json").send().await.expect(StatusCode::OK);
    let generated = serde_json::to_value(racquetcollective_server::app::openapi()).unwrap();
    assert_eq!(served, generated);
}

#[tokio::test]
async fn unknown_route_is_404() {
    let app = TestApp::spawn().await;
    let res = app.get("/nope").send().await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn api_responses_carry_security_headers() {
    // Plain HTTP (local development): everything but HSTS.
    let app = TestApp::spawn().await;
    for (path, status) in [
        ("/healthz", StatusCode::OK),
        ("/api/v1/openapi.json", StatusCode::OK),
        ("/api/v1/tenant", StatusCode::BAD_REQUEST),
        ("/api/v1/no-such-thing", StatusCode::NOT_FOUND),
        ("/nope", StatusCode::NOT_FOUND),
    ] {
        let res = app.get(path).send().await;
        assert_eq!(res.status, status, "{path}");
        assert_security_headers(&res, false);
    }

    let app = TestApp::spawn_with(Config { cookie_secure: true, ..test_config() }).await;
    let res = app.get("/healthz").send().await;
    assert_eq!(res.status, StatusCode::OK);
    assert_security_headers(&res, true);
}
