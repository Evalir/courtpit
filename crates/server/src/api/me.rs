//! `/api/v1/me`: own profile, joining a community, export and deletion.

use axum::{Json, extract::State, http::StatusCode};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, Postgres, QueryBuilder, types::Json as SqlJson};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::extract::ApiJson;
use crate::{
    ApiError, ApiResult, AppState, Tenant,
    auth::{CurrentPlayer, CurrentUser, ensure_player},
    models::{Gender, PlayPref, double_option},
    players::{self, PlayerProfile},
};

/// The signed-in user's global account.
#[derive(Debug, Serialize, ToSchema)]
pub struct Account {
    /// User id.
    pub id: Uuid,
    /// Account email.
    pub email: String,
    /// Whether the email is verified.
    pub email_verified: bool,
    /// Whether a password is set.
    pub has_password: bool,
    /// Linked sign-in providers.
    pub identities: Vec<String>,
}

/// `GET /me` response.
#[derive(Debug, Serialize, ToSchema)]
pub struct Me {
    /// Global account.
    pub account: Account,
    /// Profile in the request community.
    pub player: PlayerProfile,
}

async fn load_me(state: &AppState, player: &CurrentPlayer) -> ApiResult<Me> {
    let (has_password,): (bool,) =
        sqlx::query_as("SELECT password_hash IS NOT NULL FROM users WHERE id = $1")
            .bind(player.user.user_id)
            .fetch_one(&state.db)
            .await?;
    let identities: Vec<String> = sqlx::query_scalar(
        "SELECT provider::text FROM auth_identities WHERE user_id = $1 ORDER BY provider",
    )
    .bind(player.user.user_id)
    .fetch_all(&state.db)
    .await?;
    let mut tx = player.tenant.begin(&state.db).await?;
    let row = players::load(&mut tx, player.id)
        .await?
        .ok_or(ApiError::NotFound("player"))?;
    tx.commit().await?;
    Ok(Me {
        account: Account {
            id: player.user.user_id,
            email: player.user.email.clone(),
            email_verified: player.user.is_verified(),
            has_password,
            identities,
        },
        player: row.into(),
    })
}

/// The signed-in player's account and full profile.
#[utoipa::path(get, path = "/api/v1/me", tag = "me", security(("bearer" = [])),
    responses((status = 200, body = Me), (status = 401, body = crate::error::ErrorBody)))]
pub async fn get_me(State(state): State<AppState>, player: CurrentPlayer) -> ApiResult<Json<Me>> {
    Ok(Json(load_me(&state, &player).await?))
}

/// Partial profile update. Absent fields are untouched; `null` clears nullable fields.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProfilePatch {
    /// New display name.
    pub display_name: Option<String>,
    /// New UTR; `null` clears it.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<f64>)]
    pub utr: Option<Option<Decimal>>,
    /// New gender.
    pub gender: Option<Gender>,
    /// New phone number; `null` clears it.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub phone: Option<Option<String>>,
    /// Whether the phone number is public.
    pub phone_visible: Option<bool>,
    /// New social handles keyed by network (up to 10 strings).
    #[schema(value_type = Option<std::collections::HashMap<String, String>>)]
    pub socials: Option<Value>,
    /// Whether social handles are public.
    pub socials_visible: Option<bool>,
    /// New racket; `null` clears it.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub racket: Option<Option<String>>,
    /// New strings; `null` clears them.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub strings: Option<Option<String>>,
    /// New string tension in kg; `null` clears it.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<f64>)]
    pub tension_kg: Option<Option<Decimal>>,
    /// New match format preference.
    pub play_pref: Option<PlayPref>,
    /// New preferred locations.
    pub preferred_locations: Option<Vec<String>>,
}

fn short_text(field: &str, value: Option<String>, max: usize) -> ApiResult<Option<String>> {
    let value = value
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());
    if value
        .as_ref()
        .is_some_and(|text| text.chars().count() > max)
    {
        return Err(ApiError::validation(format!(
            "{field} must be at most {max} characters"
        )));
    }
    Ok(value)
}

/// Updates the signed-in player's profile.
#[utoipa::path(patch, path = "/api/v1/me", tag = "me", request_body = ProfilePatch,
    security(("bearer" = [])),
    responses((status = 200, body = Me), (status = 422, body = crate::error::ErrorBody)))]
pub async fn patch_me(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiJson(patch): ApiJson<ProfilePatch>,
) -> ApiResult<Json<Me>> {
    let mut qb: QueryBuilder<'_, Postgres> =
        QueryBuilder::new("UPDATE players SET updated_at = now()");
    if let Some(name) = patch.display_name {
        let name = short_text("display_name", Some(name), 60)?
            .ok_or_else(|| ApiError::validation("display_name cannot be empty"))?;
        let _ = qb.push(", display_name = ").push_bind(name);
    }
    if let Some(utr) = patch.utr {
        if let Some(rating) = utr
            && (rating < Decimal::ONE || rating > Decimal::new(1650, 2) || rating.scale() > 2)
        {
            return Err(ApiError::validation("utr must be between 1.00 and 16.50"));
        }
        let _ = qb.push(", utr = ").push_bind(utr);
    }
    if let Some(gender) = patch.gender {
        let _ = qb.push(", gender = ").push_bind(gender);
    }
    if let Some(phone) = patch.phone {
        let _ = qb
            .push(", phone = ")
            .push_bind(short_text("phone", phone, 32)?);
    }
    if let Some(visible) = patch.phone_visible {
        let _ = qb.push(", phone_visible = ").push_bind(visible);
    }
    if let Some(socials) = patch.socials {
        let ok = socials.as_object().is_some_and(|object| {
            object.len() <= 10
                && object
                    .values()
                    .all(|value| value.as_str().is_some_and(|text| text.len() <= 200))
        });
        if !ok {
            return Err(ApiError::validation(
                "socials must be an object of up to 10 strings",
            ));
        }
        let _ = qb.push(", socials = ").push_bind(SqlJson(socials));
    }
    if let Some(visible) = patch.socials_visible {
        let _ = qb.push(", socials_visible = ").push_bind(visible);
    }
    if let Some(racket) = patch.racket {
        let _ = qb
            .push(", racket = ")
            .push_bind(short_text("racket", racket, 80)?);
    }
    if let Some(strings) = patch.strings {
        let _ = qb
            .push(", strings = ")
            .push_bind(short_text("strings", strings, 80)?);
    }
    if let Some(tension) = patch.tension_kg {
        if tension.is_some_and(|kg| kg < Decimal::from(10) || kg > Decimal::from(35)) {
            return Err(ApiError::validation("tension_kg must be between 10 and 35"));
        }
        let _ = qb.push(", tension_kg = ").push_bind(tension);
    }
    if let Some(pref) = patch.play_pref {
        let _ = qb.push(", play_pref = ").push_bind(pref);
    }
    if let Some(locations) = patch.preferred_locations {
        let locations: Vec<String> = locations
            .into_iter()
            .map(|location| location.trim().to_owned())
            .filter(|location| !location.is_empty())
            .collect();
        if locations.len() > 10
            || locations
                .iter()
                .any(|location| location.chars().count() > 80)
        {
            return Err(ApiError::validation(
                "up to 10 locations of at most 80 characters",
            ));
        }
        let _ = qb
            .push(", preferred_locations = ")
            .push_bind(SqlJson(locations));
    }
    let _ = qb
        .push(" WHERE community_id = ")
        .push_bind(player.tenant.id())
        .push(" AND id = ")
        .push_bind(player.id);
    let mut tx = player.tenant.begin(&state.db).await?;
    let _ = qb.build().execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(load_me(&state, &player).await?))
}

/// Result of joining a community.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
pub struct Joined {
    /// Id of the player in this community.
    pub player_id: Uuid,
}

/// Joins the request's community with the signed-in account (open join policy).
#[utoipa::path(post, path = "/api/v1/me/join", tag = "me", security(("bearer" = [])),
    responses((status = 200, body = Joined), (status = 401, body = crate::error::ErrorBody)))]
pub async fn join(
    State(state): State<AppState>,
    tenant: Tenant,
    user: CurrentUser,
) -> ApiResult<Json<Joined>> {
    let mut tx = tenant.begin(&state.db).await?;
    let player_id = ensure_player(&mut tx, tenant.id(), user.user_id, &user.email).await?;
    let status: String =
        sqlx::query_scalar("SELECT status::text FROM players WHERE community_id = $1 AND id = $2")
            .bind(tenant.id())
            .bind(player_id)
            .fetch_one(&mut *tx)
            .await?;
    tx.commit().await?;
    if status != "active" {
        return Err(ApiError::forbidden(format!("membership is {status}")));
    }
    Ok(Json(Joined { player_id }))
}

/// The account row in an export.
#[derive(Debug, Serialize, FromRow, ToSchema)]
pub struct ExportedUser {
    /// User id.
    pub id: Uuid,
    /// Account email.
    pub email: String,
    /// When the email was verified, if it was.
    pub email_verified_at: Option<DateTime<Utc>>,
    /// Whether a password is set.
    pub has_password: bool,
    /// When the account was created.
    pub created_at: DateTime<Utc>,
}

/// A linked sign-in identity in an export.
#[derive(Debug, Serialize, FromRow, ToSchema)]
pub struct ExportedIdentity {
    /// Sign-in provider name.
    pub provider: String,
    /// Email reported by the provider, if any.
    pub email: Option<String>,
    /// When the identity was linked.
    pub created_at: DateTime<Utc>,
}

/// Everything Courtpit stores about the account (GDPR export).
#[derive(Debug, Serialize, ToSchema)]
pub struct AccountExport {
    /// When the export was generated.
    pub exported_at: DateTime<Utc>,
    /// The account row.
    pub user: ExportedUser,
    /// Linked sign-in identities.
    pub identities: Vec<ExportedIdentity>,
    /// Memberships in every community, as raw rows.
    #[schema(value_type = Vec<Object>)]
    pub memberships: Vec<Value>,
}

/// Exports the account and all memberships across communities.
///
/// This reads across tenants for the user's *own* rows, so it deliberately uses the pool
/// (privileged path) with an explicit `user_id` filter instead of a `TenantTx`.
#[utoipa::path(get, path = "/api/v1/me/export", tag = "me", security(("bearer" = [])),
    responses((status = 200, body = AccountExport), (status = 401, body = crate::error::ErrorBody)))]
pub async fn export(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<Json<AccountExport>> {
    let exported: ExportedUser = sqlx::query_as(
        "SELECT id, email, email_verified_at, password_hash IS NOT NULL AS has_password, created_at
         FROM users WHERE id = $1",
    )
    .bind(user.user_id)
    .fetch_one(&state.db)
    .await?;
    let identities: Vec<ExportedIdentity> = sqlx::query_as(
        "SELECT provider::text AS provider, email, created_at FROM auth_identities WHERE user_id = $1",
    )
    .bind(user.user_id)
    .fetch_all(&state.db)
    .await?;
    let memberships: Vec<SqlJson<Value>> = sqlx::query_scalar(
        "SELECT to_jsonb(p) || jsonb_build_object('community_slug', c.slug)
         FROM players p JOIN communities c ON c.id = p.community_id WHERE p.user_id = $1",
    )
    .bind(user.user_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(AccountExport {
        exported_at: Utc::now(),
        user: exported,
        identities,
        memberships: memberships.into_iter().map(|row| row.0).collect(),
    }))
}

/// Deletes the account: scrubs personal data from every membership (match history keeps an
/// anonymous "Deleted player"), removes identities, codes and sessions. Irreversible.
#[utoipa::path(delete, path = "/api/v1/me", tag = "me", security(("bearer" = [])),
    responses((status = 204), (status = 401, body = crate::error::ErrorBody)))]
pub async fn delete_me(State(state): State<AppState>, user: CurrentUser) -> ApiResult<StatusCode> {
    let mut tx = state.db.begin().await?;
    let _ = sqlx::query(
        "UPDATE players SET display_name = 'Deleted player', utr = NULL, gender = 'undisclosed',
            phone = NULL, phone_visible = false, socials = '{}', socials_visible = false,
            racket = NULL, strings = NULL, tension_kg = NULL, preferred_locations = '[]',
            status = 'deleted', updated_at = now()
         WHERE user_id = $1",
    )
    .bind(user.user_id)
    .execute(&mut *tx)
    .await?;
    for table in ["auth_identities", "email_codes", "sessions"] {
        let _ = sqlx::query(&format!("DELETE FROM {table} WHERE user_id = $1"))
            .bind(user.user_id)
            .execute(&mut *tx)
            .await?;
    }
    let _ = sqlx::query(
        "UPDATE users SET email = 'deleted+' || id || '@deleted.invalid', email_verified_at = NULL,
            password_hash = NULL, deleted_at = now(), updated_at = now()
         WHERE id = $1",
    )
    .bind(user.user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
