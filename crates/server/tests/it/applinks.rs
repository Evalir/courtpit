//! Universal-link and app-link association files, per community.

use axum::http::StatusCode;
use racquetcollective_server::applinks::{self, AndroidApp, AppLinks};
use serde_json::json;

use crate::{common::TestApp, web::Export};

#[tokio::test]
async fn communities_publish_the_apps_that_open_their_links() {
    // Served beside the web app: the association files win over its fallback.
    let export = Export::new();
    let app = TestApp::spawn_with(export.config()).await;
    let _ = app.community("demo").await;
    let _ = app.community("other").await;
    let links = AppLinks {
        ios: vec!["TEAM123.app.racquetcollective.demo".to_owned()],
        android: vec![AndroidApp {
            package: "app.racquetcollective.demo".to_owned(),
            sha256_cert_fingerprints: vec!["AB:CD:EF".to_owned()],
        }],
    };
    assert!(applinks::store(&app.db, "demo", &links).await.unwrap());
    assert!(!applinks::store(&app.db, "nobody", &links).await.unwrap());

    let apple = app
        .get("/.well-known/apple-app-site-association")
        .community("demo")
        .send()
        .await
        .expect(StatusCode::OK);
    let details = &apple["applinks"]["details"][0];
    assert_eq!(
        details["appIDs"],
        json!(["TEAM123.app.racquetcollective.demo"])
    );
    assert_eq!(
        details["components"][0],
        json!({ "/": "/api/*", "exclude": true, "comment": "The API is not a page" })
    );
    assert_eq!(details["components"][1], json!({ "/": "*" }));
    let android = app
        .get("/.well-known/assetlinks.json")
        .community("demo")
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(
        android,
        json!([{
            "relation": ["delegate_permission/common.handle_all_urls"],
            "target": {
                "namespace": "android_app",
                "package_name": "app.racquetcollective.demo",
                "sha256_cert_fingerprints": ["AB:CD:EF"],
            },
        }])
    );

    // Without apps, links keep opening the web app.
    for path in [
        "/.well-known/apple-app-site-association",
        "/.well-known/assetlinks.json",
    ] {
        let _ = app
            .get(path)
            .community("other")
            .send()
            .await
            .expect(StatusCode::NOT_FOUND);
    }
}
