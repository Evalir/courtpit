//! Player rows and their public (redacted) projection.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, types::Json};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    TenantTx,
    models::{Gender, PlayPref, PlayerRole, PlayerStatus},
};

/// Columns for [`PlayerRow`]; expects `players p JOIN users u`.
pub const PLAYER_COLUMNS: &str = "p.id, p.user_id, p.display_name, p.utr, p.gender, p.phone, \
    p.phone_visible, p.socials, p.socials_visible, p.racket, p.strings, p.tension_kg, \
    p.play_pref, p.preferred_locations, p.role, p.status, p.created_at, \
    (u.email_verified_at IS NOT NULL) AS email_verified";

/// A full player row (tenant-scoped; read inside a `TenantTx`).
#[derive(Debug, Clone, FromRow)]
pub struct PlayerRow {
    /// Player id.
    pub id: Uuid,
    /// Owning user account id.
    pub user_id: Uuid,
    /// Name shown to other players.
    pub display_name: String,
    /// Universal Tennis Rating, if set.
    pub utr: Option<Decimal>,
    /// Self-declared gender.
    pub gender: Gender,
    /// Contact phone number, if set.
    pub phone: Option<String>,
    /// Whether the phone number is shown to other players.
    pub phone_visible: bool,
    /// Social handles keyed by network.
    pub socials: Json<Value>,
    /// Whether social handles are shown to other players.
    pub socials_visible: bool,
    /// Racket model, if set.
    pub racket: Option<String>,
    /// String setup, if set.
    pub strings: Option<String>,
    /// String tension in kilograms, if set.
    pub tension_kg: Option<Decimal>,
    /// Preferred match format.
    pub play_pref: PlayPref,
    /// Places the player likes to play.
    pub preferred_locations: Json<Vec<String>>,
    /// Role within the community.
    pub role: PlayerRole,
    /// Membership status.
    pub status: PlayerStatus,
    /// When the player joined.
    pub created_at: DateTime<Utc>,
    /// Whether the account email is verified.
    pub email_verified: bool,
}

/// Loads one player of the transaction's community.
pub async fn load(tx: &mut TenantTx, id: Uuid) -> Result<Option<PlayerRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {PLAYER_COLUMNS} FROM players p JOIN users u ON u.id = p.user_id
         WHERE p.community_id = $1 AND p.id = $2"
    ))
    .bind(tx.community_id())
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
}

/// The owner's own view of their profile: everything, including gender and contact details.
#[derive(Debug, Serialize, ToSchema)]
pub struct PlayerProfile {
    /// Player id.
    pub id: Uuid,
    /// Name shown to other players.
    pub display_name: String,
    /// Universal Tennis Rating, if set.
    #[schema(value_type = Option<f64>)]
    pub utr: Option<Decimal>,
    /// Self-declared gender.
    pub gender: Gender,
    /// Contact phone number, if set.
    pub phone: Option<String>,
    /// Whether the phone number is shown to other players.
    pub phone_visible: bool,
    /// Social handles keyed by network.
    #[schema(value_type = Object)]
    pub socials: Value,
    /// Whether social handles are shown to other players.
    pub socials_visible: bool,
    /// Racket model, if set.
    pub racket: Option<String>,
    /// String setup, if set.
    pub strings: Option<String>,
    /// String tension in kilograms, if set.
    #[schema(value_type = Option<f64>)]
    pub tension_kg: Option<Decimal>,
    /// Preferred match format.
    pub play_pref: PlayPref,
    /// Places the player likes to play.
    pub preferred_locations: Vec<String>,
    /// Role within the community.
    pub role: PlayerRole,
    /// Membership status.
    pub status: PlayerStatus,
    /// When the player joined.
    pub created_at: DateTime<Utc>,
}

impl From<PlayerRow> for PlayerProfile {
    fn from(row: PlayerRow) -> Self {
        Self {
            id: row.id,
            display_name: row.display_name,
            utr: row.utr,
            gender: row.gender,
            phone: row.phone,
            phone_visible: row.phone_visible,
            socials: row.socials.0,
            socials_visible: row.socials_visible,
            racket: row.racket,
            strings: row.strings,
            tension_kg: row.tension_kg,
            play_pref: row.play_pref,
            preferred_locations: row.preferred_locations.0,
            role: row.role,
            status: row.status,
            created_at: row.created_at,
        }
    }
}
