//! Universal links (iOS) and app links (Android): the association files that let a community's
//! app open `https://{its host}/...` links itself (spec §14). Each community ships its own app,
//! so the files are per host, from `settings.app_links`:
//!
//! ```json
//! { "app_links": {
//!     "ios": ["ABCDE12345.app.racquetcollective.riverside"],
//!     "android": [{ "package": "app.racquetcollective.riverside",
//!                   "sha256_cert_fingerprints": ["AB:CD:…"] }] } }
//! ```
//!
//! A community without them answers 404, and links keep opening the web app.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::{ApiError, Tenant};

/// The apps allowed to open a community's links.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppLinks {
    /// iOS app ids: `{team id}.{bundle id}`.
    pub ios: Vec<String>,
    /// Android apps by package and signing-certificate fingerprints.
    pub android: Vec<AndroidApp>,
}

/// One Android app allowed to open the community's links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AndroidApp {
    /// The application id.
    pub package: String,
    /// SHA-256 fingerprints of the signing certificates (`AB:CD:…`).
    pub sha256_cert_fingerprints: Vec<String>,
}

impl AppLinks {
    /// The `app_links` of a community's settings (none when absent or malformed).
    pub fn of(tenant: &Tenant) -> Self {
        tenant
            .settings
            .0
            .get("app_links")
            .and_then(|links| serde_json::from_value(links.clone()).ok())
            .unwrap_or_default()
    }
}

/// Stores the apps allowed to open a community's links (the `set-app-links` command). Returns
/// whether the community exists. Running servers see it once their tenant cache expires.
pub async fn store(db: &PgPool, slug: &str, links: &AppLinks) -> anyhow::Result<bool> {
    let updated = sqlx::query(
        "UPDATE communities SET settings = jsonb_set(settings, '{app_links}', $2, true)
         WHERE slug = $1",
    )
    .bind(slug)
    .bind(sqlx::types::Json(links))
    .execute(db)
    .await?
    .rows_affected();
    Ok(updated == 1)
}

/// Paths the app opens: everything but the API.
fn components() -> Value {
    json!([
        { "/": "/api/*", "exclude": true, "comment": "The API is not a page" },
        { "/": "*" },
    ])
}

/// `GET /.well-known/apple-app-site-association`.
pub async fn apple(tenant: Tenant) -> Result<Response, ApiError> {
    let links = AppLinks::of(&tenant);
    if links.ios.is_empty() {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    Ok(Json(json!({
        "applinks": { "details": [{ "appIDs": links.ios, "components": components() }] },
        "webcredentials": { "apps": links.ios },
    }))
    .into_response())
}

/// `GET /.well-known/assetlinks.json`.
pub async fn android(tenant: Tenant) -> Result<Response, ApiError> {
    let links = AppLinks::of(&tenant);
    if links.android.is_empty() {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let statements: Vec<Value> = links
        .android
        .iter()
        .map(|app| {
            json!({
                "relation": ["delegate_permission/common.handle_all_urls"],
                "target": {
                    "namespace": "android_app",
                    "package_name": app.package,
                    "sha256_cert_fingerprints": app.sha256_cert_fingerprints,
                },
            })
        })
        .collect();
    Ok(Json(statements).into_response())
}
