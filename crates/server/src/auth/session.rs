//! Sessions and the `CurrentUser` / `CurrentPlayer` / `ClientIp` extractors.

use std::net::IpAddr;

use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, request::Parts},
};
use axum_extra::{
    extract::cookie::CookieJar,
    headers::{Authorization, HeaderMapExt, authorization::Bearer},
};
use chrono::{DateTime, Duration, Utc};
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

use crate::{
    ApiError, AppState, Tenant,
    auth::secrets::{hash_token, new_session_token},
    models::{PlayerRole, PlayerStatus},
};

/// Cookie carrying the session token on web.
pub const SESSION_COOKIE: &str = "racquetcollective_session";
/// `X-RacquetCollective-Client: web` asks for the session as an httpOnly cookie instead of in the
/// body.
pub const CLIENT_HEADER: &str = "x-racquetcollective-client";

/// How a new session reaches the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionDelivery {
    /// An httpOnly cookie, for the web app (it asks with [`CLIENT_HEADER`]).
    Cookie,
    /// The token in the response body, for native apps.
    Body,
}

impl SessionDelivery {
    /// [`SessionDelivery::Cookie`] when the client sent `X-RacquetCollective-Client: web`.
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let web = headers
            .get(CLIENT_HEADER)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.eq_ignore_ascii_case("web"));
        if web { Self::Cookie } else { Self::Body }
    }
}

/// Creates a session; returns the plaintext token (never stored) and its expiry.
pub async fn create_session(
    db: &PgPool,
    user_id: Uuid,
    device_label: Option<&str>,
    ttl: Duration,
) -> Result<(String, DateTime<Utc>), sqlx::Error> {
    let (token, hash) = new_session_token();
    let expires_at = Utc::now() + ttl;
    let _ = sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, device_label, expires_at) VALUES ($1, $2, $3, $4)",
    )
    .bind(hash)
    .bind(user_id)
    .bind(device_label.map(|label| label.chars().take(100).collect::<String>()))
    .bind(expires_at)
    .execute(db)
    .await?;
    Ok((token, expires_at))
}

/// Ensures the user has a membership in the transaction's community (open join policy).
/// Returns the player id.
pub async fn ensure_player(
    conn: &mut PgConnection,
    community_id: Uuid,
    user_id: Uuid,
    email: &str,
) -> Result<Uuid, sqlx::Error> {
    let display_name =
        email.split('@').next().filter(|local| !local.is_empty()).unwrap_or("Player");
    let _ = sqlx::query(
        "INSERT INTO players (id, community_id, user_id, display_name) VALUES ($1, $2, $3, $4)
         ON CONFLICT (community_id, user_id) DO NOTHING",
    )
    .bind(Uuid::now_v7())
    .bind(community_id)
    .bind(user_id)
    .bind(display_name)
    .execute(&mut *conn)
    .await?;
    sqlx::query_scalar("SELECT id FROM players WHERE community_id = $1 AND user_id = $2")
        .bind(community_id)
        .bind(user_id)
        .fetch_one(&mut *conn)
        .await
}

fn bearer_or_cookie(headers: &HeaderMap) -> Option<String> {
    if let Some(Authorization(bearer)) = headers.typed_get::<Authorization<Bearer>>() {
        return Some(bearer.token().to_owned());
    }
    CookieJar::from_headers(headers).get(SESSION_COOKIE).map(|cookie| cookie.value().to_owned())
}

/// The authenticated user (global identity), from a bearer token or the session cookie.
#[derive(Debug, Clone, FromRow)]
pub struct CurrentUser {
    /// The user's id.
    pub user_id: Uuid,
    /// The user's email address.
    pub email: String,
    /// When the email was verified, if it has been.
    pub email_verified_at: Option<DateTime<Utc>>,
    /// Hash of the session token that authenticated this request.
    #[sqlx(skip)]
    pub session_hash: Vec<u8>,
}

impl CurrentUser {
    /// Whether the user's email is verified.
    pub const fn is_verified(&self) -> bool {
        self.email_verified_at.is_some()
    }
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        if let Some(user) = parts.extensions.get::<Self>() {
            return Ok(user.clone());
        }
        let token = bearer_or_cookie(&parts.headers).ok_or(ApiError::Unauthorized)?;
        let hash = hash_token(&token);
        let user: Option<Self> = sqlx::query_as(
            "SELECT u.id AS user_id, u.email, u.email_verified_at
             FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.token_hash = $1 AND s.expires_at > now() AND u.deleted_at IS NULL",
        )
        .bind(&hash)
        .fetch_optional(&state.db)
        .await?;
        let mut user = user.ok_or(ApiError::Unauthorized)?;
        user.session_hash = hash;
        let _ = parts.extensions.insert(user.clone());
        Ok(user)
    }
}

#[derive(Debug, Clone, FromRow)]
struct PlayerRow {
    id: Uuid,
    display_name: String,
    role: PlayerRole,
    status: PlayerStatus,
}

/// The authenticated user's active membership in the request's community. Taking this
/// extractor is the membership cross-check the spec requires before scoped queries run.
#[derive(Debug, Clone)]
pub struct CurrentPlayer {
    /// The player (membership) id.
    pub id: Uuid,
    /// Name shown to other members.
    pub display_name: String,
    /// The player's role in the community.
    pub role: PlayerRole,
    /// The authenticated user behind the membership.
    pub user: CurrentUser,
    /// The request's community.
    pub tenant: Tenant,
}

impl CurrentPlayer {
    /// Errors unless the player is a community admin or owner.
    pub fn require_admin(&self) -> Result<(), ApiError> {
        if self.role.is_admin() {
            Ok(())
        } else {
            Err(ApiError::forbidden("community admin role required"))
        }
    }

    /// Errors unless the user's email is verified (needed to be listed or to register).
    pub fn require_verified(&self) -> Result<(), ApiError> {
        if self.user.is_verified() {
            Ok(())
        } else {
            Err(ApiError::forbidden("verify your email first"))
        }
    }
}

impl FromRequestParts<AppState> for CurrentPlayer {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let tenant = Tenant::from_request_parts(parts, state).await?;
        let user = CurrentUser::from_request_parts(parts, state).await?;
        let mut tx = tenant.begin(&state.db).await?;
        let row: Option<PlayerRow> = sqlx::query_as(
            "SELECT id, display_name, role, status FROM players
             WHERE community_id = $1 AND user_id = $2",
        )
        .bind(tenant.id())
        .bind(user.user_id)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        let row = row.ok_or_else(|| ApiError::forbidden("not a member of this community"))?;
        match row.status {
            PlayerStatus::Active => {}
            PlayerStatus::Banned => return Err(ApiError::forbidden("banned from this community")),
            PlayerStatus::Deleted => return Err(ApiError::Unauthorized),
        }
        Ok(Self { id: row.id, display_name: row.display_name, role: row.role, user, tenant })
    }
}

/// Client IP for rate limiting, from the source configured by `RACQUETCOLLECTIVE_CLIENT_IP_SOURCE`
/// (see [`axum_client_ip::ClientIpSource`]). Rejections render as our usual error body.
#[derive(Debug, Clone, Copy)]
pub struct ClientIp(pub IpAddr);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let axum_client_ip::ClientIp(ip) =
            axum_client_ip::ClientIp::from_request_parts(parts, state)
                .await
                .map_err(|err| ApiError::Internal(err.into()))?;
        Ok(Self(ip))
    }
}
