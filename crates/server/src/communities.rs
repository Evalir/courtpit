//! Creating communities (used by the `create-community` subcommand and tests).

use std::collections::BTreeMap;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, types::Json};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::tenancy::{Community, TenantTx};

/// Theme stored in `communities.branding`; clients apply it at boot. Missing keys take their
/// defaults; unknown keys are ignored so newer branding doesn't break older instances.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct Branding {
    /// Name shown in the app; clients fall back to the community name when absent.
    pub display_name: Option<String>,
    /// Logo image URL.
    pub logo_url: Option<String>,
    /// Color tokens by name (e.g. `primary`, `background`), as CSS color strings.
    pub colors: BTreeMap<String, String>,
    /// Typography choice: a font family key the client knows.
    pub typography: Option<String>,
    /// Feature flags by name.
    pub feature_flags: BTreeMap<String, bool>,
}

/// Input for [`create_community`].
#[derive(Debug, Clone)]
pub struct NewCommunity {
    /// URL-safe identifier; trimmed and lowercased.
    pub slug: String,
    /// Display name.
    pub name: String,
    /// Optional custom domain; trimmed and lowercased.
    pub custom_domain: Option<String>,
    /// Initial theme.
    pub branding: Branding,
    /// Optional owner: an existing or new user, made verified and given an `owner` player row.
    pub owner_email: Option<String>,
}

/// Result of [`create_community`].
#[derive(Debug, Clone)]
pub struct CreatedCommunity {
    /// The inserted community.
    pub community: Community,
    /// The owner's player row, when an owner was requested.
    pub owner_player_id: Option<Uuid>,
}

/// Inserts a community and, optionally, its owner.
pub async fn create_community(db: &PgPool, new: NewCommunity) -> anyhow::Result<CreatedCommunity> {
    let community: Community = sqlx::query_as(
        "INSERT INTO communities (id, slug, name, custom_domain, branding)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, slug, name, custom_domain, branding, settings, scoring_config,
                   default_match_format, created_at",
    )
    .bind(Uuid::now_v7())
    .bind(new.slug.trim().to_ascii_lowercase())
    .bind(new.name.trim())
    .bind(
        new.custom_domain
            .map(|domain| domain.trim().to_ascii_lowercase()),
    )
    .bind(Json(&new.branding))
    .fetch_one(db)
    .await
    .context("inserting community (slug must be lowercase letters, digits and dashes)")?;

    let owner_player_id = if let Some(email) = new.owner_email {
        let email = email.trim().to_owned();
        let user_id: Uuid = sqlx::query_scalar(
            "INSERT INTO users (id, email, email_verified_at) VALUES ($1, $2, now())
             ON CONFLICT (lower(email)) DO UPDATE
                SET email_verified_at = coalesce(users.email_verified_at, now())
             RETURNING id",
        )
        .bind(Uuid::now_v7())
        .bind(&email)
        .fetch_one(db)
        .await
        .context("upserting owner user")?;
        let display_name = email.split('@').next().unwrap_or("Owner").to_owned();
        let mut tx = TenantTx::begin(db, community.id).await?;
        let player_id: Uuid = sqlx::query_scalar(
            "INSERT INTO players (id, community_id, user_id, display_name, role)
             VALUES ($1, $2, $3, $4, 'owner') RETURNING id",
        )
        .bind(Uuid::now_v7())
        .bind(community.id)
        .bind(user_id)
        .bind(display_name)
        .fetch_one(&mut *tx)
        .await
        .context("inserting owner player")?;
        tx.commit().await?;
        Some(player_id)
    } else {
        None
    };
    Ok(CreatedCommunity {
        community,
        owner_player_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branding_defaults_missing_keys_and_ignores_unknown_ones() {
        use serde_json::json;
        let empty: Branding = serde_json::from_value(json!({})).unwrap();
        assert_eq!(empty, Branding::default());
        let branding: Branding = serde_json::from_value(json!({
            "display_name": "Demo",
            "colors": { "primary": "#0a7" },
            "feature_flags": { "doubles": true },
            "future_key": 1,
        }))
        .unwrap();
        assert_eq!(branding.display_name.as_deref(), Some("Demo"));
        assert_eq!(branding.colors["primary"], "#0a7");
        assert!(branding.feature_flags["doubles"]);
        assert_eq!(branding.logo_url, None);
    }
}
