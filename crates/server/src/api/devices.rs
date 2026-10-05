//! `/api/v1/me/devices` (push tokens) and `/api/v1/me/notifications` (what to be told).

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    ApiError, ApiResult, AppState,
    auth::CurrentPlayer,
    extract::{ApiJson, ApiPath},
    notify::NotificationPrefs,
};

/// The operating system a push token belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[sqlx(type_name = "device_platform", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum DevicePlatform {
    /// iPhone and iPad.
    Ios,
    /// Android phones and tablets.
    Android,
}

/// Body of `PUT /me/devices/{token}`.
#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterDevice {
    /// The device's operating system.
    pub platform: DevicePlatform,
}

/// Expo push tokens look like `ExponentPushToken[...]` (or `ExpoPushToken[...]`).
fn check_token(token: &str) -> ApiResult<()> {
    let wrapped =
        ["ExponentPushToken[", "ExpoPushToken["].iter().any(|prefix| token.starts_with(prefix));
    if wrapped && token.ends_with(']') && token.len() <= 255 {
        Ok(())
    } else {
        Err(ApiError::validation("not an Expo push token"))
    }
}

/// Registers (or refreshes) this device's push token for the signed-in player. A token another
/// member registered on this device moves to the caller.
#[utoipa::path(put, path = "/api/v1/me/devices/{token}", tag = "me",
    params(("token" = String, Path, description = "The device's Expo push token")),
    request_body = RegisterDevice, security(("bearer" = [])),
    responses((status = 204), (status = 422, body = crate::error::ErrorBody)))]
pub async fn register_device(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath(token): ApiPath<String>,
    ApiJson(body): ApiJson<RegisterDevice>,
) -> ApiResult<StatusCode> {
    check_token(&token)?;
    let mut tx = player.tenant.begin(&state.db).await?;
    let _ = sqlx::query(
        "INSERT INTO device_tokens (id, community_id, player_id, expo_push_token, platform)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (community_id, expo_push_token) DO UPDATE
            SET player_id = excluded.player_id, platform = excluded.platform,
                last_seen_at = now()",
    )
    .bind(Uuid::now_v7())
    .bind(tx.community_id())
    .bind(player.id)
    .bind(&token)
    .bind(body.platform)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Stops pushing to this device (signing out). Unknown tokens are fine.
#[utoipa::path(delete, path = "/api/v1/me/devices/{token}", tag = "me",
    params(("token" = String, Path, description = "The device's Expo push token")),
    security(("bearer" = [])), responses((status = 204)))]
pub async fn forget_device(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath(token): ApiPath<String>,
) -> ApiResult<StatusCode> {
    let mut tx = player.tenant.begin(&state.db).await?;
    let _ = sqlx::query(
        "DELETE FROM device_tokens
         WHERE community_id = $1 AND player_id = $2 AND expo_push_token = $3",
    )
    .bind(tx.community_id())
    .bind(player.id)
    .bind(&token)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The signed-in player's notification preferences.
#[utoipa::path(get, path = "/api/v1/me/notifications", tag = "me", security(("bearer" = [])),
    responses((status = 200, body = NotificationPrefs)))]
pub async fn get_notifications(
    State(state): State<AppState>,
    player: CurrentPlayer,
) -> ApiResult<Json<NotificationPrefs>> {
    let mut tx = player.tenant.begin(&state.db).await?;
    let prefs: Option<NotificationPrefs> = sqlx::query_as(
        "SELECT match_updates, league_updates, reminders FROM notification_prefs
         WHERE community_id = $1 AND player_id = $2",
    )
    .bind(tx.community_id())
    .bind(player.id)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(prefs.unwrap_or_default()))
}

/// Sets the signed-in player's notification preferences.
#[utoipa::path(put, path = "/api/v1/me/notifications", tag = "me",
    request_body = NotificationPrefs, security(("bearer" = [])),
    responses((status = 200, body = NotificationPrefs)))]
pub async fn set_notifications(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiJson(prefs): ApiJson<NotificationPrefs>,
) -> ApiResult<Json<NotificationPrefs>> {
    let mut tx = player.tenant.begin(&state.db).await?;
    let _ = sqlx::query(
        "INSERT INTO notification_prefs
            (player_id, community_id, match_updates, league_updates, reminders)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (player_id) DO UPDATE
            SET match_updates = excluded.match_updates,
                league_updates = excluded.league_updates,
                reminders = excluded.reminders, updated_at = now()",
    )
    .bind(player.id)
    .bind(tx.community_id())
    .bind(prefs.match_updates)
    .bind(prefs.league_updates)
    .bind(prefs.reminders)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(prefs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_expo_tokens_are_accepted() {
        check_token("ExponentPushToken[xxxxxxxxxxxxxxxxxxxxxx]").unwrap();
        check_token("ExpoPushToken[abc]").unwrap();
        let _ = check_token("fcm:abc").unwrap_err();
        let _ = check_token("ExponentPushToken[abc").unwrap_err();
    }
}
