//! The exported web app served beside the API (`RACQUETCOLLECTIVE_WEB_DIR`).

use std::{fs, path::PathBuf};

use axum::http::{StatusCode, header};
use racquetcollective_server::{Config, web::WebApp};
use uuid::Uuid;

use crate::common::{Res, TestApp, test_config};

/// A throwaway export: the app shell, a hashed bundle with its gzip twin, and a favicon.
pub(crate) struct Export(PathBuf);

impl Export {
    pub(crate) fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("racquetcollective-web-{}", Uuid::now_v7()));
        let bundles = dir.join("_expo/static/js/web");
        fs::create_dir_all(&bundles).unwrap();
        fs::write(dir.join("index.html"), "<!doctype html><title>app</title>").unwrap();
        fs::write(dir.join("favicon.ico"), "icon").unwrap();
        fs::write(bundles.join("entry-abc123.js"), "console.log(1)").unwrap();
        fs::write(bundles.join("entry-abc123.js.gz"), "gzipped").unwrap();
        Self(dir)
    }

    pub(crate) fn config(&self) -> Config {
        Config {
            web_dir: Some(self.0.clone()),
            ..test_config()
        }
    }
}

impl Drop for Export {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn header_of(res: &Res, name: header::HeaderName) -> &str {
    res.headers
        .get(name)
        .map_or("", |value| value.to_str().unwrap())
}

#[tokio::test]
async fn app_routes_get_the_shell_and_files_are_served_as_they_are() {
    let export = Export::new();
    let app = TestApp::spawn_with(export.config()).await;

    for path in ["/", "/leagues/01a10927", "/verify?email=ana%40example.com"] {
        let res = app.get(path).send().await;
        assert_eq!(res.status, StatusCode::OK, "{path}");
        assert!(header_of(&res, header::CONTENT_TYPE).starts_with("text/html"));
        assert_eq!(header_of(&res, header::CACHE_CONTROL), "no-cache");
        assert!(res.body.as_str().unwrap().contains("<title>app"));
    }

    let bundle = "/_expo/static/js/web/entry-abc123.js";
    let res = app.get(bundle).send().await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(
        header_of(&res, header::CACHE_CONTROL),
        "public, max-age=31536000, immutable"
    );
    assert_eq!(res.body, "console.log(1)");
    let res = app
        .get(bundle)
        .header("accept-encoding", "gzip")
        .send()
        .await;
    assert_eq!(header_of(&res, header::CONTENT_ENCODING), "gzip");
    assert_eq!(res.body, "gzipped");

    let res = app.get("/favicon.ico").send().await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(header_of(&res, header::CACHE_CONTROL), "no-cache");

    // A missing file is a 404, not the shell (a stale bundle URL must not cache HTML as JS),
    // and nothing outside the export is reachable.
    for path in ["/_expo/static/js/web/entry-old.js", "/..%2fCargo.toml"] {
        let res = app.get(path).send().await;
        assert_eq!(res.status, StatusCode::NOT_FOUND, "{path}");
        assert!(header_of(&res, header::CACHE_CONTROL).is_empty());
    }
}

#[tokio::test]
async fn the_api_keeps_its_paths_and_error_shape() {
    let export = Export::new();
    let app = TestApp::spawn_with(export.config()).await;
    let _ = app.community("demo").await;

    let tenant = app
        .get("/api/v1/tenant")
        .community("demo")
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(tenant["slug"], "demo");
    let health = app.get("/healthz").send().await.expect(StatusCode::OK);
    assert_eq!(health["status"], "ok");
    let missing = app
        .get("/api/v1/no-such-thing")
        .send()
        .await
        .expect(StatusCode::NOT_FOUND);
    assert_eq!(missing["error"]["code"], "not_found");
}

#[tokio::test]
async fn without_a_web_dir_only_the_api_answers() {
    let app = TestApp::spawn().await;
    let _ = app.get("/").send().await.expect(StatusCode::NOT_FOUND);
    let missing = app
        .get("/api/v2/anything")
        .send()
        .await
        .expect(StatusCode::NOT_FOUND);
    assert_eq!(missing["error"]["code"], "not_found");
}

#[test]
fn a_directory_without_an_index_is_refused_at_startup() {
    let export = Export::new();
    fs::remove_file(export.0.join("index.html")).unwrap();
    let err = WebApp::new(&export.0).unwrap_err();
    assert!(err.to_string().contains("index.html"), "{err}");
}
