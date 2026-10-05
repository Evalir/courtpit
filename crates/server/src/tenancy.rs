//! Tenant resolution and tenant-scoped transactions.
//!
//! Every handler that touches community data takes a [`Tenant`]; every tenant-scoped query runs
//! inside a [`TenantTx`], which scopes the transaction to the community *and* drops to the
//! `courtpit_app` role so Postgres row-level security is enforced.

use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
    time::Duration,
};

use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, header, request::Parts},
};
use chrono::{DateTime, Utc};
use moka::future::Cache;
use serde_json::Value;
use sqlx::{FromRow, PgConnection, PgPool, Postgres, Transaction, types::Json};
use uuid::Uuid;

use crate::{ApiError, AppState, communities::Branding};

/// Header carrying the community slug (native clients bake it into the build).
pub const COMMUNITY_HEADER: &str = "x-racquetcollective-community";

/// A community (tenant) row.
#[derive(Debug, Clone, FromRow)]
pub struct Community {
    /// Community id.
    pub id: Uuid,
    /// URL-safe identifier (`{slug}.{base_domain}`, tenant header).
    pub slug: String,
    /// Display name.
    pub name: String,
    /// Registered custom domain, lowercase.
    pub custom_domain: Option<String>,
    /// Client theme.
    pub branding: Json<Branding>,
    /// Community settings.
    pub settings: Json<Value>,
    /// Points and ranking rules.
    pub scoring_config: Json<Value>,
    /// Match format used when a match doesn't pick one.
    pub default_match_format: Json<Value>,
    /// When the community was created.
    pub created_at: DateTime<Utc>,
}

const COMMUNITY_COLUMNS: &str = "id, slug, name, custom_domain, branding, settings, \
    scoring_config, default_match_format, created_at";

/// Where a request said its tenant is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenantKey {
    /// Explicit slug (header or `{slug}.{base_domain}` host).
    Slug(String),
    /// A registered custom domain.
    Domain(String),
}

/// Extracts the tenant key from request headers. The header wins over the host.
pub fn tenant_key(headers: &HeaderMap, base_domain: &str) -> Option<TenantKey> {
    if let Some(slug) = headers
        .get(COMMUNITY_HEADER)
        .and_then(|value| value.to_str().ok())
    {
        let slug = slug.trim().to_ascii_lowercase();
        if !slug.is_empty() {
            return Some(TenantKey::Slug(slug));
        }
    }
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())?;
    let host = host
        .split(':')
        .next()
        .unwrap_or(host)
        .trim()
        .to_ascii_lowercase();
    let base = base_domain.trim_start_matches('.').to_ascii_lowercase();
    if host.is_empty() || host == base {
        return None;
    }
    if let Some(prefix) = host.strip_suffix(&format!(".{base}")) {
        return (!prefix.contains('.')).then(|| TenantKey::Slug(prefix.to_owned()));
    }
    // Bare hosts like `localhost` carry no tenant.
    host.contains('.').then_some(TenantKey::Domain(host))
}

/// Cache of communities by slug / custom domain.
#[derive(Clone)]
pub struct TenantCache {
    cache: Cache<String, Arc<Community>>,
}

impl std::fmt::Debug for TenantCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TenantCache").finish_non_exhaustive()
    }
}

impl TenantCache {
    /// A cache whose entries expire after `ttl` (so branding edits propagate).
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(ttl)
                .build(),
        }
    }

    /// Resolves a key to a community, hitting the database on a miss. Misses aren't cached.
    pub async fn resolve(
        &self,
        db: &PgPool,
        key: &TenantKey,
    ) -> Result<Option<Arc<Community>>, ApiError> {
        let cache_key = match key {
            TenantKey::Slug(slug) => format!("slug:{slug}"),
            TenantKey::Domain(domain) => format!("domain:{domain}"),
        };
        if let Some(hit) = self.cache.get(&cache_key).await {
            return Ok(Some(hit));
        }
        let (column, value) = match key {
            TenantKey::Slug(slug) => ("slug", slug),
            TenantKey::Domain(domain) => ("custom_domain", domain),
        };
        let sql = format!("SELECT {COMMUNITY_COLUMNS} FROM communities WHERE {column} = $1");
        let found: Option<Community> = sqlx::query_as(&sql).bind(value).fetch_optional(db).await?;
        Ok(match found {
            Some(community) => {
                let community = Arc::new(community);
                self.cache.insert(cache_key, Arc::clone(&community)).await;
                Some(community)
            }
            None => None,
        })
    }

    /// Drops all cached entries (after a community is edited).
    pub fn invalidate_all(&self) {
        self.cache.invalidate_all();
    }
}

/// The community this request is scoped to.
#[derive(Debug, Clone)]
pub struct Tenant(pub Arc<Community>);

impl Tenant {
    /// Loads a tenant by id, for work that runs outside a request (jobs).
    pub async fn load(db: &PgPool, id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        let sql = format!("SELECT {COMMUNITY_COLUMNS} FROM communities WHERE id = $1");
        let found: Option<Community> = sqlx::query_as(&sql).bind(id).fetch_optional(db).await?;
        Ok(found.map(|community| Self(Arc::new(community))))
    }

    /// The community id.
    pub fn id(&self) -> Uuid {
        self.0.id
    }

    /// Opens a transaction scoped to this community.
    pub async fn begin(&self, db: &PgPool) -> Result<TenantTx, sqlx::Error> {
        TenantTx::begin(db, self.0.id).await
    }
}

impl Deref for Tenant {
    type Target = Community;
    fn deref(&self) -> &Community {
        &self.0
    }
}

impl FromRequestParts<AppState> for Tenant {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        if let Some(tenant) = parts.extensions.get::<Self>() {
            return Ok(tenant.clone());
        }
        let key = tenant_key(&parts.headers, &state.config.base_domain).ok_or_else(|| {
            ApiError::BadRequest(format!(
                "no community: send the `{COMMUNITY_HEADER}` header or use a community host"
            ))
        })?;
        let community = state
            .tenants
            .resolve(&state.db, &key)
            .await?
            .ok_or(ApiError::NotFound("community"))?;
        let tenant = Self(community);
        let _ = parts.extensions.insert(tenant.clone());
        Ok(tenant)
    }
}

/// A transaction scoped to one community, running as the RLS-bound `courtpit_app` role.
///
/// Derefs to a connection: use `.fetch_one(&mut *tx)`. Dropped without [`commit`](Self::commit)
/// it rolls back.
#[derive(Debug)]
pub struct TenantTx {
    tx: Transaction<'static, Postgres>,
    community_id: Uuid,
}

impl TenantTx {
    /// Begins a transaction, sets `app.community_id` and switches to `courtpit_app`.
    pub async fn begin(db: &PgPool, community_id: Uuid) -> Result<Self, sqlx::Error> {
        let mut tx = db.begin().await?;
        let _ = sqlx::query("SELECT set_config('app.community_id', $1, true)")
            .bind(community_id.to_string())
            .execute(&mut *tx)
            .await?;
        let _ = sqlx::query("SET LOCAL ROLE courtpit_app")
            .execute(&mut *tx)
            .await?;
        Ok(Self { tx, community_id })
    }

    /// The community this transaction is scoped to.
    pub const fn community_id(&self) -> Uuid {
        self.community_id
    }

    /// Commits the transaction.
    pub async fn commit(self) -> Result<(), sqlx::Error> {
        self.tx.commit().await
    }
}

impl Deref for TenantTx {
    type Target = PgConnection;
    fn deref(&self) -> &PgConnection {
        &self.tx
    }
}

impl DerefMut for TenantTx {
    fn deref_mut(&mut self) -> &mut PgConnection {
        &mut self.tx
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            let _ = map.insert(*name, HeaderValue::from_static(value));
        }
        map
    }

    #[test]
    fn header_wins_over_host() {
        let map = headers(&[
            (COMMUNITY_HEADER, "Demo"),
            ("host", "other.racquetcollective.app"),
        ]);
        assert_eq!(
            tenant_key(&map, "racquetcollective.app"),
            Some(TenantKey::Slug("demo".into()))
        );
    }

    #[test]
    fn subdomain_of_base_is_slug() {
        let map = headers(&[("host", "madrid.racquetcollective.app:443")]);
        assert_eq!(
            tenant_key(&map, "racquetcollective.app"),
            Some(TenantKey::Slug("madrid".into()))
        );
    }

    #[test]
    fn nested_subdomain_or_bare_base_is_none() {
        assert_eq!(
            tenant_key(
                &headers(&[("host", "a.b.racquetcollective.app")]),
                "racquetcollective.app"
            ),
            None
        );
        assert_eq!(
            tenant_key(
                &headers(&[("host", "racquetcollective.app")]),
                "racquetcollective.app"
            ),
            None
        );
        assert_eq!(
            tenant_key(
                &headers(&[("host", "localhost:8080")]),
                "racquetcollective.app"
            ),
            None
        );
    }

    #[test]
    fn foreign_host_is_custom_domain() {
        let map = headers(&[("host", "Tennis.Example.org")]);
        assert_eq!(
            tenant_key(&map, "racquetcollective.app"),
            Some(TenantKey::Domain("tennis.example.org".into()))
        );
    }
}
