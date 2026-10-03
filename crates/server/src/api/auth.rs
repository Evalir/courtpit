//! `/api/v1/auth/*`: one-time email codes, passwords, sessions.

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{DateTime, Duration, Utc};
use email_address::{EmailAddress, Options};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

use subtle::ConstantTimeEq;

use crate::extract::ApiJson;
use crate::{
    ApiError, ApiResult, AppState, Tenant,
    auth::{
        ClientIp, CurrentPlayer, CurrentUser, SESSION_COOKIE, create_session, ensure_player,
        secrets::{hash_code, new_code},
        wants_cookie,
    },
    mailer::Email,
    models::PlayerRole,
};

/// One-time codes expire after this long.
const CODE_TTL_MINUTES: i64 = 10;
/// Wrong guesses allowed per code before it is burned.
const MAX_CODE_ATTEMPTS: i32 = 5;
/// Codes a single address may request per hour.
const CODES_PER_EMAIL_PER_HOUR: i64 = 5;
const MIN_PASSWORD_LEN: usize = 10;
const MAX_PASSWORD_LEN: usize = 256;

/// Trims and validates an email address (RFC 5322 addr-spec with a dotted domain).
pub(crate) fn normalize_email(raw: &str) -> Result<String, ApiError> {
    let options = Options::default()
        .with_required_tld()
        .without_display_text()
        .without_domain_literal();
    EmailAddress::parse_with_options(raw.trim(), options)
        .map(|email| email.as_str().to_owned())
        .map_err(|_| ApiError::validation("invalid email address"))
}

/// Body of `POST /auth/otp/request`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct OtpRequest {
    /// Address to send the code to.
    pub email: String,
}

/// Generic acknowledgement.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
pub struct Accepted {
    /// What happened, e.g. `"sent"`.
    pub status: &'static str,
}

/// Emails a 6-digit sign-in code. Always answers 202 for valid input (no account enumeration).
#[utoipa::path(
    post,
    path = "/api/v1/auth/otp/request",
    tag = "auth",
    request_body = OtpRequest,
    responses(
        (status = 202, body = Accepted),
        (status = 422, body = crate::error::ErrorBody),
        (status = 429, body = crate::error::ErrorBody),
    )
)]
pub async fn request_otp(
    State(state): State<AppState>,
    tenant: Tenant,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<OtpRequest>,
) -> ApiResult<(StatusCode, Json<Accepted>)> {
    let email = normalize_email(&body.email)?;
    state.limiter.check(&format!("otp-ip:{ip}"))?;

    let mut tx = state.db.begin().await?;
    let (user_id, verified_at): (Uuid, Option<DateTime<Utc>>) = sqlx::query_as(
        "INSERT INTO users (id, email) VALUES ($1, $2)
         ON CONFLICT (lower(email)) DO UPDATE SET updated_at = users.updated_at
         RETURNING id, email_verified_at",
    )
    .bind(Uuid::now_v7())
    .bind(&email)
    .fetch_one(&mut *tx)
    .await?;
    let recent: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM email_codes WHERE user_id = $1 AND created_at > now() - interval '1 hour'",
    )
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    if recent >= CODES_PER_EMAIL_PER_HOUR {
        return Err(ApiError::RateLimited);
    }
    // Only the newest code is valid.
    let _ = sqlx::query(
        "UPDATE email_codes SET consumed_at = now() WHERE user_id = $1 AND consumed_at IS NULL",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    let code_id = Uuid::now_v7();
    let code = new_code();
    let purpose = if verified_at.is_some() {
        "login"
    } else {
        "verify"
    };
    let _ = sqlx::query(
        "INSERT INTO email_codes (id, user_id, code_hash, purpose, expires_at)
         VALUES ($1, $2, $3, $4::email_code_purpose, now() + make_interval(mins => $5))",
    )
    .bind(code_id)
    .bind(user_id)
    .bind(hash_code(code_id, &code))
    .bind(purpose)
    .bind(CODE_TTL_MINUTES as i32)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    state
        .mailer
        .send(&Email {
            to: email,
            subject: format!("Your {} sign-in code", tenant.name),
            text: format!(
                "Your {} code is {code}. It expires in {CODE_TTL_MINUTES} minutes.\n\
                 If you didn't ask for it, ignore this email.",
                tenant.name
            ),
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(Accepted { status: "sent" })))
}

/// Body of `POST /auth/otp/verify`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct OtpVerify {
    /// Address the code was sent to.
    pub email: String,
    /// The emailed 6-digit code.
    pub code: String,
    /// Free-form label shown in session lists ("Ana's iPhone").
    pub device_label: Option<String>,
}

/// A new session. `token` is omitted when it was delivered as a cookie
/// (`X-Courtpit-Client: web`).
#[derive(Debug, Serialize, ToSchema)]
pub struct AuthSession {
    /// Bearer token for the session.
    pub token: Option<String>,
    /// When the session stops being valid.
    pub expires_at: DateTime<Utc>,
    /// The signed-in user.
    pub user_id: Uuid,
    /// The user's membership in the request's community.
    pub player_id: Uuid,
}

#[derive(FromRow)]
struct CodeRow {
    id: Uuid,
    code_hash: Vec<u8>,
    attempts: i32,
    expires_at: DateTime<Utc>,
}

/// Exchanges an emailed code for a session; verifies the email and joins the community.
#[utoipa::path(
    post,
    path = "/api/v1/auth/otp/verify",
    tag = "auth",
    request_body = OtpVerify,
    responses(
        (status = 200, body = AuthSession),
        (status = 401, body = crate::error::ErrorBody),
        (status = 429, body = crate::error::ErrorBody),
    )
)]
pub async fn verify_otp(
    State(state): State<AppState>,
    tenant: Tenant,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    ApiJson(body): ApiJson<OtpVerify>,
) -> ApiResult<Response> {
    state.limiter.check(&format!("verify-ip:{ip}"))?;
    let email = normalize_email(&body.email)?;
    let mut tx = state.db.begin().await?;
    let user: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, email FROM users WHERE lower(email) = lower($1) AND deleted_at IS NULL",
    )
    .bind(&email)
    .fetch_optional(&mut *tx)
    .await?;
    let (user_id, email) = user.ok_or(ApiError::InvalidCredentials)?;
    let code: Option<CodeRow> = sqlx::query_as(
        "SELECT id, code_hash, attempts, expires_at FROM email_codes
         WHERE user_id = $1 AND consumed_at IS NULL ORDER BY created_at DESC LIMIT 1 FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let code_row = code.ok_or(ApiError::InvalidCredentials)?;
    if code_row.expires_at <= Utc::now() || code_row.attempts >= MAX_CODE_ATTEMPTS {
        return Err(ApiError::InvalidCredentials);
    }
    let matches: bool = hash_code(code_row.id, &body.code)
        .ct_eq(&code_row.code_hash)
        .into();
    if !matches {
        let _ = sqlx::query(
            "UPDATE email_codes SET attempts = attempts + 1,
                consumed_at = CASE WHEN attempts + 1 >= $2 THEN now() END
             WHERE id = $1",
        )
        .bind(code_row.id)
        .bind(MAX_CODE_ATTEMPTS)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Err(ApiError::InvalidCredentials);
    }
    let _ = sqlx::query("UPDATE email_codes SET consumed_at = now() WHERE id = $1")
        .bind(code_row.id)
        .execute(&mut *tx)
        .await?;
    let _ = sqlx::query(
        "UPDATE users SET email_verified_at = coalesce(email_verified_at, now()), updated_at = now()
         WHERE id = $1",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    finish_login(
        &state,
        &tenant,
        user_id,
        &email,
        body.device_label.as_deref(),
        &headers,
    )
    .await
}

/// Joins the community if needed, opens a session and renders it (cookie or body).
pub(crate) async fn finish_login(
    state: &AppState,
    tenant: &Tenant,
    user_id: Uuid,
    email: &str,
    device_label: Option<&str>,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    let mut tx = tenant.begin(&state.db).await?;
    let player_id = ensure_player(&mut tx, tenant.id(), user_id, email).await?;
    tx.commit().await?;
    let ttl = Duration::days(state.config.session_ttl_days);
    let (token, expires_at) = create_session(&state.db, user_id, device_label, ttl).await?;
    let cookie = wants_cookie(headers);
    let body = Json(AuthSession {
        token: (!cookie).then(|| token.clone()),
        expires_at,
        user_id,
        player_id,
    });
    if !cookie {
        return Ok(body.into_response());
    }
    let jar = CookieJar::new().add(
        Cookie::build((SESSION_COOKIE, token))
            .path("/")
            .http_only(true)
            .same_site(SameSite::Lax)
            .max_age(cookie::time::Duration::seconds(ttl.num_seconds()))
            .secure(state.config.cookie_secure),
    );
    Ok((jar, body).into_response())
}

/// Body of `POST /auth/password/login`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PasswordLogin {
    /// The account's email address.
    pub email: String,
    /// The account's password.
    pub password: String,
    /// Free-form label shown in session lists.
    pub device_label: Option<String>,
}

#[derive(FromRow)]
struct LoginRow {
    id: Uuid,
    email: String,
    password_hash: Option<String>,
    email_verified_at: Option<DateTime<Utc>>,
}

/// Signs in with email and password (only for verified users who set one).
#[utoipa::path(
    post,
    path = "/api/v1/auth/password/login",
    tag = "auth",
    request_body = PasswordLogin,
    responses(
        (status = 200, body = AuthSession),
        (status = 401, body = crate::error::ErrorBody),
        (status = 429, body = crate::error::ErrorBody),
    )
)]
pub async fn password_login(
    State(state): State<AppState>,
    tenant: Tenant,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    ApiJson(body): ApiJson<PasswordLogin>,
) -> ApiResult<Response> {
    let email = normalize_email(&body.email)?;
    state.limiter.check(&format!("login-ip:{ip}"))?;
    state
        .limiter
        .check(&format!("login-email:{}", email.to_lowercase()))?;
    let user: Option<LoginRow> = sqlx::query_as(
        "SELECT id, email, password_hash, email_verified_at FROM users
         WHERE lower(email) = lower($1) AND deleted_at IS NULL",
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await?;
    let Some(LoginRow {
        id: user_id,
        email,
        password_hash: Some(phc),
        email_verified_at: Some(_),
    }) = user
    else {
        return Err(ApiError::InvalidCredentials);
    };
    let password = body.password;
    tokio::task::spawn_blocking(move || password_auth::verify_password(password, &phc))
        .await
        .map_err(|err| ApiError::Internal(err.into()))?
        .map_err(|_| ApiError::InvalidCredentials)?;
    finish_login(
        &state,
        &tenant,
        user_id,
        &email,
        body.device_label.as_deref(),
        &headers,
    )
    .await
}

/// Body of `PUT /auth/password`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SetPassword {
    /// 10–256 characters.
    pub password: String,
}

/// Sets or replaces the signed-in user's password. The session (from a code) is the proof.
#[utoipa::path(
    put,
    path = "/api/v1/auth/password",
    tag = "auth",
    request_body = SetPassword,
    security(("bearer" = [])),
    responses((status = 204), (status = 401, body = crate::error::ErrorBody), (status = 422, body = crate::error::ErrorBody))
)]
pub async fn set_password(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(body): ApiJson<SetPassword>,
) -> ApiResult<StatusCode> {
    let len = body.password.chars().count();
    if !(MIN_PASSWORD_LEN..=MAX_PASSWORD_LEN).contains(&len) {
        return Err(ApiError::validation(format!(
            "password must be {MIN_PASSWORD_LEN}-{MAX_PASSWORD_LEN} characters"
        )));
    }
    let password = body.password;
    let phc = tokio::task::spawn_blocking(move || password_auth::generate_hash(password))
        .await
        .map_err(|err| ApiError::Internal(err.into()))?;
    let _ = sqlx::query("UPDATE users SET password_hash = $2, updated_at = now() WHERE id = $1")
        .bind(user.user_id)
        .bind(phc)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Ends the current session (row delete) and clears the cookie.
#[utoipa::path(
    post,
    path = "/api/v1/auth/logout",
    tag = "auth",
    security(("bearer" = [])),
    responses((status = 204), (status = 401, body = crate::error::ErrorBody))
)]
pub async fn logout(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<(CookieJar, StatusCode)> {
    let _ = sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(&user.session_hash)
        .execute(&state.db)
        .await?;
    let jar = CookieJar::new().remove(Cookie::build(SESSION_COOKIE).path("/"));
    Ok((jar, StatusCode::NO_CONTENT))
}

/// Who the session belongs to in this community.
#[derive(Debug, Serialize, ToSchema)]
pub struct SessionInfo {
    /// The signed-in user.
    pub user_id: Uuid,
    /// The user's email address.
    pub email: String,
    /// Whether the email has been verified.
    pub email_verified: bool,
    /// The user's membership in this community.
    pub player_id: Uuid,
    /// The player's role in this community.
    pub role: PlayerRole,
}

/// Describes the current session in the request's community.
#[utoipa::path(
    get,
    path = "/api/v1/auth/session",
    tag = "auth",
    security(("bearer" = [])),
    responses((status = 200, body = SessionInfo), (status = 401, body = crate::error::ErrorBody), (status = 403, body = crate::error::ErrorBody))
)]
pub async fn session(player: CurrentPlayer) -> Json<SessionInfo> {
    Json(SessionInfo {
        email_verified: player.user.is_verified(),
        user_id: player.user.user_id,
        email: player.user.email,
        player_id: player.id,
        role: player.role,
    })
}
