//! `POST /api/v1/auth/oidc/{provider}` (sign in) and `.../link` (attach to current user).

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::extract::{ApiJson, ApiPath};
use crate::{
    ApiError, ApiResult, AppState, Tenant,
    api::auth::finish_login,
    auth::{
        ClientIp, CurrentUser,
        oidc::{IdClaims, Provider},
    },
};

/// Body for OIDC sign-in and linking.
#[derive(Debug, Deserialize, ToSchema)]
pub struct OidcLogin {
    /// The provider's ID token (JWT).
    pub id_token: String,
    /// The nonce the client put in the authorization request, if any (must match the token).
    pub nonce: Option<String>,
    /// Optional label for the device, stored with the new session.
    pub device_label: Option<String>,
}

async fn verified_claims(
    state: &AppState,
    provider: &str,
    body: &OidcLogin,
) -> ApiResult<(Provider, IdClaims)> {
    let provider: Provider = provider.parse()?;
    let claims = state
        .oidc
        .verify(provider, &body.id_token, body.nonce.as_deref())
        .await?;
    Ok((provider, claims))
}

/// Signs in with an Apple or Google ID token. Links to an existing account when the provider
/// vouches for the same (verified) email; otherwise creates one.
#[utoipa::path(
    post,
    path = "/api/v1/auth/oidc/{provider}",
    tag = "auth",
    params(("provider" = String, Path, description = "`apple` or `google`")),
    request_body = OidcLogin,
    responses(
        (status = 200, body = crate::api::auth::AuthSession),
        (status = 401, body = crate::error::ErrorBody),
        (status = 409, description = "Email belongs to an account not linked to this identity", body = crate::error::ErrorBody),
        (status = 429, body = crate::error::ErrorBody),
    )
)]
pub async fn oidc_login(
    State(state): State<AppState>,
    tenant: Tenant,
    ClientIp(ip): ClientIp,
    ApiPath(provider): ApiPath<String>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<OidcLogin>,
) -> ApiResult<Response> {
    state.limiter.check(&format!("login-ip:{ip}"))?;
    let (provider, claims) = verified_claims(&state, &provider, &body).await?;

    let mut tx = state.db.begin().await?;
    let linked: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT u.id, u.email FROM auth_identities i JOIN users u ON u.id = i.user_id
         WHERE i.provider = $1 AND i.subject = $2 AND u.deleted_at IS NULL",
    )
    .bind(provider)
    .bind(&claims.subject)
    .fetch_optional(&mut *tx)
    .await?;
    let (user_id, email) = if let Some(found) = linked {
        found
    } else {
        let email = claims
            .email
            .clone()
            .ok_or_else(|| ApiError::validation("the provider did not share an email address"))?;
        let existing: Option<(Uuid, String)> = sqlx::query_as(
            "SELECT id, email FROM users WHERE lower(email) = lower($1) AND deleted_at IS NULL",
        )
        .bind(&email)
        .fetch_optional(&mut *tx)
        .await?;
        let user = match existing {
            Some(user) if claims.email_verified => user,
            // Never attach an unverified provider email to someone else's account.
            Some(_) => {
                return Err(ApiError::conflict(
                    "an account with this email exists; sign in with a code, then link",
                ));
            }
            None => {
                let id = Uuid::now_v7();
                let _ = sqlx::query("INSERT INTO users (id, email) VALUES ($1, $2)")
                    .bind(id)
                    .bind(&email)
                    .execute(&mut *tx)
                    .await?;
                (id, email)
            }
        };
        insert_identity(&mut tx, user.0, provider, &claims).await?;
        user
    };
    if claims.email_verified
        && claims
            .email
            .as_deref()
            .is_some_and(|stored| stored.eq_ignore_ascii_case(&email))
    {
        let _ = sqlx::query(
            "UPDATE users SET email_verified_at = coalesce(email_verified_at, now()) WHERE id = $1",
        )
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    }
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

async fn insert_identity(
    conn: &mut sqlx::PgConnection,
    user_id: Uuid,
    provider: Provider,
    claims: &IdClaims,
) -> ApiResult<()> {
    let inserted = sqlx::query(
        "INSERT INTO auth_identities (id, user_id, provider, subject, email) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (provider, subject) DO NOTHING",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(provider)
    .bind(&claims.subject)
    .bind(&claims.email)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if inserted == 0 {
        let owner: Uuid = sqlx::query_scalar(
            "SELECT user_id FROM auth_identities WHERE provider = $1 AND subject = $2",
        )
        .bind(provider)
        .bind(&claims.subject)
        .fetch_one(&mut *conn)
        .await?;
        if owner != user_id {
            return Err(ApiError::conflict(
                "this identity is linked to another account",
            ));
        }
    }
    Ok(())
}

/// Links an Apple or Google identity to the signed-in user.
#[utoipa::path(
    post,
    path = "/api/v1/auth/oidc/{provider}/link",
    tag = "auth",
    params(("provider" = String, Path, description = "`apple` or `google`")),
    request_body = OidcLogin,
    security(("bearer" = [])),
    responses(
        (status = 204),
        (status = 401, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody),
    )
)]
pub async fn oidc_link(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiPath(provider): ApiPath<String>,
    ApiJson(body): ApiJson<OidcLogin>,
) -> ApiResult<StatusCode> {
    let (provider, claims) = verified_claims(&state, &provider, &body).await?;
    let mut tx = state.db.begin().await?;
    insert_identity(&mut tx, user.user_id, provider, &claims).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
