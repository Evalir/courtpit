//! Postgres enum types shared across handlers.

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
