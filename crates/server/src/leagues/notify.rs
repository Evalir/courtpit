//! Emails about league lifecycle events, sent by jobs so they happen after the transaction
//! that caused them committed and retry on failure.

use sqlx::FromRow;
use uuid::Uuid;

use crate::{AppState, TenantTx, mailer::Email};

#[derive(Debug, FromRow)]
struct Recipient {
    email: String,
    display_name: String,
    league_name: String,
    cancel_reason: Option<String>,
}

/// The job: tells `player_id` that `league_id` was cancelled. Players without a verified
/// email, deleted accounts and leagues that are not cancelled are skipped, so retries and
/// duplicate deliveries are harmless apart from a repeated email after a send that succeeded
/// but was not acknowledged.
pub async fn league_cancelled(
    state: &AppState,
    community_id: Uuid,
    league_id: Uuid,
    player_id: Uuid,
) -> anyhow::Result<()> {
    let mut tx = TenantTx::begin(&state.db, community_id).await?;
    let recipient: Option<Recipient> = sqlx::query_as(
        "SELECT u.email, p.display_name, l.name AS league_name, l.cancel_reason
         FROM leagues l, players p JOIN users u ON u.id = p.user_id
         WHERE l.community_id = $1 AND l.id = $2 AND l.status = 'cancelled'
           AND p.community_id = $1 AND p.id = $3 AND p.status <> 'deleted'
           AND u.email_verified_at IS NOT NULL",
    )
    .bind(community_id)
    .bind(league_id)
    .bind(player_id)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    let Some(recipient) = recipient else {
        return Ok(());
    };
    let reason =
        recipient.cancel_reason.map(|reason| format!("\n\nReason: {reason}")).unwrap_or_default();
    let email = Email {
        to: recipient.email,
        subject: format!("League cancelled: {}", recipient.league_name),
        text: format!(
            "Hi {},\n\nThe league \"{}\" has been cancelled and will not be played.{reason}\n\n\
             Your entry is closed and there is nothing you need to do.",
            recipient.display_name, recipient.league_name
        ),
    };
    state.mailer.send(&email).await?;
    tracing::info!(%league_id, %player_id, "league cancellation emailed");
    Ok(())
}
