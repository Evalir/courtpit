//! Postgres enum types shared across handlers.
#![expect(
    clippy::option_if_let_else,
    reason = "fires inside utoipa's `ToSchema` derive for the generic `Page<T>`"
)]

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// A player's role in a community.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[sqlx(type_name = "player_role", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum PlayerRole {
    /// A regular member.
    Player,
    /// May manage the community.
    Admin,
    /// Owns the community; has every admin right.
    Owner,
}

impl PlayerRole {
    /// Admins and owners may act as community admins.
    pub const fn is_admin(self) -> bool {
        matches!(self, Self::Admin | Self::Owner)
    }
}

/// Membership status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[sqlx(type_name = "player_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum PlayerStatus {
    /// A current member in good standing.
    Active,
    /// Barred from the community by an admin.
    Banned,
    /// The membership was removed.
    Deleted,
}

/// Self-declared gender, used only to validate mixed-doubles eligibility. Never shown publicly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[sqlx(type_name = "player_gender", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Gender {
    /// Female.
    Female,
    /// Male.
    Male,
    /// Another gender.
    Other,
    /// Prefers not to say.
    Undisclosed,
}

impl From<Gender> for racquetcollective_domain::Gender {
    fn from(gender: Gender) -> Self {
        match gender {
            Gender::Female => Self::Female,
            Gender::Male => Self::Male,
            Gender::Other => Self::Other,
            Gender::Undisclosed => Self::Undisclosed,
        }
    }
}

/// What a player likes to play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[sqlx(type_name = "play_pref", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum PlayPref {
    /// Prefers singles.
    Singles,
    /// Prefers doubles.
    Doubles,
    /// No preference.
    Any,
}

/// A page of results with an opaque cursor for the next page.
#[derive(Debug, Serialize, ToSchema)]
pub struct Page<T> {
    /// Items on this page.
    pub items: Vec<T>,
    /// Pass as `cursor` to fetch the next page; absent on the last page.
    pub next_cursor: Option<String>,
}

/// Standard `?cursor=&limit=` query parameters.
#[derive(Debug, Clone, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageParams {
    /// Opaque cursor from a previous page.
    pub cursor: Option<String>,
    /// Page size, 1–100 (default 20).
    pub limit: Option<i64>,
}

impl PageParams {
    /// The clamped page size.
    pub fn limit(&self) -> i64 {
        self.limit.unwrap_or(20).clamp(1, 100)
    }

    /// Decodes a UUID cursor (ids are v7, so ordering by id is ordering by creation).
    pub fn uuid_cursor(&self) -> Result<Option<uuid::Uuid>, crate::ApiError> {
        self.cursor
            .as_deref()
            .map(|cursor| {
                cursor
                    .parse()
                    .map_err(|_| crate::ApiError::BadRequest("invalid cursor".into()))
            })
            .transpose()
    }
}

/// Builds a page from `limit + 1` fetched rows, using `key` for the cursor.
pub fn paginate<T>(mut rows: Vec<T>, limit: i64, key: impl Fn(&T) -> String) -> Page<T> {
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    let next_cursor = if rows.len() > limit {
        rows.truncate(limit);
        rows.last().map(key)
    } else {
        None
    };
    Page {
        items: rows,
        next_cursor,
    }
}

/// Deserializes a present field (even `null`) as `Some(..)`, so PATCH bodies can tell
/// "absent" (`None`) from "set to null" (`Some(None)`).
pub fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}
