//! The exported web app (`expo export --platform web` in `apps/mobile`), served by the API from
//! the same origin (decision 77): the session cookie and host-based community resolution work
//! unchanged, and there is no CORS. Paths that name a file are served from the export; every
//! other path outside `/api` is one of the app's routes and gets its `index.html`.

use std::path::{Path, PathBuf};

use axum::{
    extract::Request,
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};

/// Expo names bundles and assets by content hash, so they never change under one URL.
const HASHED: [&str; 2] = ["/_expo/static/", "/assets/"];

/// The export in one directory, ready to answer requests.
#[derive(Debug, Clone)]
pub struct WebApp {
    files: ServeDir,
    index: ServeFile,
}

impl WebApp {
    /// Serves the export in `dir`, preferring the `.gz` twin of a file when the client accepts
    /// gzip (the image build precompresses them). Errors unless `dir` holds an `index.html`.
    pub fn new(dir: &Path) -> anyhow::Result<Self> {
        let index: PathBuf = dir.join("index.html");
        anyhow::ensure!(
            index.is_file(),
            "COURTPIT_WEB_DIR {} has no index.html (run `npm run export:web` in apps/mobile)",
            dir.display()
        );
        Ok(Self {
            files: ServeDir::new(dir).precompressed_gzip(),
            index: ServeFile::new(index).precompressed_gzip(),
        })
    }

    /// Answers a request outside the API: the file it names, else the app's `index.html`.
    /// Hashed bundles are cached for a year; everything else revalidates on every load, so a
    /// deploy reaches users on their next visit.
    pub async fn respond(self, request: Request) -> Response {
        let path = request.uri().path().to_owned();
        let names_file = path
            .rsplit('/')
            .next()
            .is_some_and(|segment| segment.contains('.'));
        // A missing file is a 404, never the app shell: a stale bundle URL must not cache
        // index.html as JavaScript.
        let mut response = if names_file {
            self.files.oneshot(request).await.into_response()
        } else {
            self.index.oneshot(request).await.into_response()
        };
        if response.status().is_success() {
            let cache = if HASHED.iter().any(|prefix| path.starts_with(prefix)) {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            };
            let _ = response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
        }
        response
    }
}
