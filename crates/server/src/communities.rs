//! Creating communities (used by the `create-community` subcommand and tests).

use anyhow::Context;
use serde_json::Value;
use sqlx::{PgPool, types::Json};
use uuid::Uuid;

use crate::tenancy::{Community, TenantTx};

/// Input for [`create_community`].
#[derive(Debug, Clone)]
pub struct NewCommunity {
    pub slug: String,
    pub name: String,
    pub custom_domain: Option<String>,
    pub branding: Value,
    /// Optional owner: an existing or new user, made verified and given an `owner` player row.
    pub owner_email: Option<String>,
}

/// Result of [`create_community`].
#[derive(Debug, Clone)]
pub struct CreatedCommunity {
    pub community: Community,
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
    .bind(new.custom_domain.map(|d| d.trim().to_ascii_lowercase()))
    .bind(Json(&new.branding))
    .fetch_one(db)
    .await
    .context("inserting community (slug must be lowercase letters, digits and dashes)")?;

    let mut owner_player_id = None;
    if let Some(email) = new.owner_email {
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
        owner_player_id = Some(player_id);
    }
    Ok(CreatedCommunity {
        community,
        owner_player_id,
    })
}
