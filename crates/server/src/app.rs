//! Router assembly and shared application state.

use std::{sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::Request,
    http::HeaderName,
    routing::{any, get},
};
use sqlx::PgPool;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use utoipa::{Modify, OpenApi};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    ApiError, api,
    auth::{oidc::OidcVerifier, rate_limit::RateLimiter},
    backup::Backup,
    clock::{Clock, SystemClock},
    config::{Config, MailerKind},
    mailer::{LogMailer, Mailer, ResendMailer},
    openapi::{ApiConventions, ApiDoc},
    tenancy::TenantCache,
    web::WebApp,
};

/// State shared by every handler. Cheap to clone.
#[derive(Debug, Clone)]
pub struct AppState {
    /// Runtime configuration.
    pub config: Arc<Config>,
    /// Postgres pool. Tenant-scoped queries must go through `TenantTx`.
    pub db: PgPool,
    /// Communities by slug / custom domain.
    pub tenants: TenantCache,
    /// Outbound email.
    pub mailer: Arc<dyn Mailer>,
    /// Per-IP / per-key auth rate limits (`auth_ip_limit_per_hour`).
    pub limiter: RateLimiter,
    /// Apple / Google ID-token verification.
    pub oidc: OidcVerifier,
    /// Source of "now" for deadlines, schedules and jobs (tests move it).
    pub clock: Arc<dyn Clock>,
    /// Where nightly database backups come from and go to; `None` when not configured.
    pub backup: Option<Backup>,
}

impl AppState {
    /// Builds state with an explicit mailer (tests pass a shared [`LogMailer`]).
    pub fn new(config: Config, db: PgPool, mailer: Arc<dyn Mailer>) -> Self {
        let tenants = TenantCache::new(Duration::from_secs(config.tenant_cache_ttl_secs));
        let limiter = RateLimiter::per_hour(config.auth_ip_limit_per_hour);
        let oidc = OidcVerifier::from_config(&config);
        Self {
            oidc,
            config: Arc::new(config),
            db,
            tenants,
            mailer,
            limiter,
            clock: Arc::new(SystemClock),
            backup: None,
        }
    }

    /// Replaces the clock (tests use [`crate::clock::OffsetClock`]).
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Sets (or clears) the backup dump source and object store (tests pass fakes).
    #[must_use]
    pub fn with_backup(mut self, backup: Option<Backup>) -> Self {
        self.backup = backup;
        self
    }

    /// Builds state, choosing the mailer and the backup target from configuration.
    pub fn from_config(config: Config, db: PgPool) -> anyhow::Result<Self> {
        let mailer: Arc<dyn Mailer> = match config.mailer {
            MailerKind::Log => Arc::new(LogMailer::default()),
            MailerKind::Resend => {
                let key = config.resend_api_key.clone().ok_or_else(|| {
                    anyhow::anyhow!("RESEND_API_KEY is required with --mailer resend")
                })?;
                Arc::new(ResendMailer::new(key, config.email_from.clone()))
            }
        };
        let backup = Backup::from_config(&config)?;
        Ok(Self::new(config, db, mailer).with_backup(backup))
    }
}

/// All API routes plus the OpenAPI document describing them.
#[expect(
    clippy::cognitive_complexity,
    reason = "a flat list of route registrations; the `routes!` expansion inflates the score"
)]
pub fn api_router() -> (Router<AppState>, utoipa::openapi::OpenApi) {
    let (router, mut doc) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(api::health::healthz))
        .routes(routes!(api::health::readyz))
        .routes(routes!(api::tenant::get_tenant))
        .routes(routes!(api::auth::request_otp))
        .routes(routes!(api::auth::verify_otp))
        .routes(routes!(api::auth::password_login))
        .routes(routes!(api::auth::set_password))
        .routes(routes!(api::auth::logout))
        .routes(routes!(api::auth::session))
        .routes(routes!(api::oidc::oidc_login))
        .routes(routes!(api::oidc::oidc_link))
        .routes(routes!(
            api::me::get_me,
            api::me::patch_me,
            api::me::delete_me
        ))
        .routes(routes!(api::me::join))
        .routes(routes!(api::me::export))
        .routes(routes!(api::players::list_players))
        .routes(routes!(api::players::get_player))
        .routes(routes!(api::players::ban_player))
        .routes(routes!(api::players::unban_player))
        .routes(routes!(
            api::matches::create_match,
            api::matches::list_matches
        ))
        .routes(routes!(api::matches::get_match))
        .routes(routes!(api::matches::cancel_match))
        .routes(routes!(api::proposals::propose))
        .routes(routes!(api::proposals::accept_proposal))
        .routes(routes!(api::proposals::decline_proposal))
        .routes(routes!(api::match_results::report_score))
        .routes(routes!(api::match_results::confirm_score))
        .routes(routes!(api::match_results::dispute_score))
        .routes(routes!(api::match_results::resolve_match))
        .routes(routes!(api::match_results::walkover_match))
        .routes(routes!(
            api::match_requests::create_request,
            api::match_requests::list_requests
        ))
        .routes(routes!(api::match_requests::get_request))
        .routes(routes!(api::match_requests::join_request))
        .routes(routes!(api::match_requests::leave_request))
        .routes(routes!(api::match_requests::cancel_request))
        .routes(routes!(api::leagues::create_league))
        .routes(routes!(api::leagues::patch_league))
        .routes(routes!(api::leagues::publish_league))
        .routes(routes!(api::leagues::cancel_league))
        .routes(routes!(api::leagues::finish_league))
        .routes(routes!(api::leagues::unresolved_matches))
        .routes(routes!(api::leagues::list_leagues))
        .routes(routes!(api::leagues::get_league))
        .routes(routes!(api::leagues::league_standings))
        .routes(routes!(
            api::league_entries::register,
            api::league_entries::list_entries
        ))
        .routes(routes!(api::league_entries::accept_invite))
        .routes(routes!(api::league_entries::decline_invite))
        .routes(routes!(api::league_entries::withdraw))
        .routes(routes!(api::league_entries::pair_entries))
        .routes(routes!(api::rankings::list_rankings))
        .routes(routes!(api::rankings::ledger))
        .split_for_parts();
    ApiConventions.modify(&mut doc);
    (router, doc)
}

/// The OpenAPI document of [`api_router`]. Served at `/api/v1/openapi.json` and printed by
/// `courtpit-server openapi`, so the committed client and the live API cannot disagree.
pub fn openapi() -> utoipa::openapi::OpenApi {
    api_router().1
}

/// Unknown API paths answer in the API's error shape, never with the web app.
async fn no_such_route() -> ApiError {
    ApiError::NotFound("route")
}

/// The complete application router, ready to serve. With a `web` app, every path outside the
/// API serves it (see [`WebApp`]).
pub fn router(state: AppState, web: Option<WebApp>) -> Router {
    let (api, openapi) = api_router();
    let client_ip_source = state.config.client_ip_source.clone();
    let openapi = Arc::new(openapi);
    let request_id = HeaderName::from_static("x-request-id");
    let api = api
        .route(
            "/api/v1/openapi.json",
            get(move || {
                let doc = Arc::clone(&openapi);
                async move { Json(doc.as_ref().clone()) }
            }),
        )
        .route("/api/{*path}", any(no_such_route));
    let app = match web {
        Some(web) => api.fallback(move |request: Request| web.clone().respond(request)),
        None => api,
    };
    app.layer(PropagateRequestIdLayer::new(request_id.clone()))
        .layer(TraceLayer::new_for_http())
        .layer(SetRequestIdLayer::new(request_id, MakeRequestUuid))
        .layer(client_ip_source.into_extension())
        .with_state(state)
}
