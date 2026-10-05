//! Notifications (spec §14): what players are told and in which category, queued as jobs by
//! the handlers that cause them, and the job that tells them: a push to each of their devices,
//! or an email when they have none and the news needs an answer.

use chrono::{DateTime, Utc};
use courtpit_domain::{MatchStatus, Score, Side};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    AppState, Tenant, TenantTx,
    communities::CommunitySettings,
    jobs::{self, Job},
    mailer::Email,
    matches::{self, MatchRow},
    players::Names,
    push::{Delivery, PushMessage},
};

/// Something a player is told about. Only ids: the words are written when the job runs, from
/// the state at that moment, so a late notification never shows stale names or scores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// `by` proposed a time for the match.
    ProposalReceived {
        /// The match.
        match_id: Uuid,
        /// Who proposed.
        by: Uuid,
    },
    /// `by` accepted the recipient's side's proposal: the match is scheduled.
    ProposalAccepted {
        /// The match.
        match_id: Uuid,
        /// Who accepted.
        by: Uuid,
    },
    /// `by` declined the recipient's side's proposal.
    ProposalDeclined {
        /// The match.
        match_id: Uuid,
        /// Who declined.
        by: Uuid,
    },
    /// `by` reported a score for the recipient to confirm or dispute.
    ScoreReported {
        /// The match.
        match_id: Uuid,
        /// Who reported.
        by: Uuid,
    },
    /// `by` disputed the score the recipient's side reported.
    ScoreDisputed {
        /// The match.
        match_id: Uuid,
        /// Who disputed.
        by: Uuid,
    },
    /// An admin decided the match: a ruling, a replay, a walkover or a cancellation.
    MatchDecided {
        /// The match.
        match_id: Uuid,
    },
    /// `by` challenged the recipient to a friendly.
    Challenged {
        /// The new match.
        match_id: Uuid,
        /// Who challenged.
        by: Uuid,
    },
    /// A match request the recipient is in filled up and became this match.
    RequestFilled {
        /// The new match.
        match_id: Uuid,
    },
    /// `by` cancelled a friendly with the recipient.
    MatchCancelled {
        /// The match.
        match_id: Uuid,
        /// Who cancelled.
        by: Uuid,
    },
}

/// The groups a player can turn off (`notification_prefs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Proposals, scores, disputes and rulings on the player's matches.
    MatchUpdates,
    /// Partner invitations and league news.
    LeagueUpdates,
    /// Reminders before scheduled matches.
    Reminders,
}

impl Event {
    /// The category the player's preferences gate this event under.
    pub const fn category(self) -> Category {
        match self {
            Self::ProposalReceived { .. }
            | Self::ProposalAccepted { .. }
            | Self::ProposalDeclined { .. }
            | Self::ScoreReported { .. }
            | Self::ScoreDisputed { .. }
            | Self::MatchDecided { .. }
            | Self::Challenged { .. }
            | Self::RequestFilled { .. }
            | Self::MatchCancelled { .. } => Category::MatchUpdates,
        }
    }

    /// Whether a player without a device gets this by email: it asks for an answer before a
    /// deadline (an unanswered score confirms itself).
    pub const fn emails_without_devices(self) -> bool {
        matches!(self, Self::ScoreReported { .. })
    }

    const fn match_id(self) -> Uuid {
        match self {
            Self::ProposalReceived { match_id, .. }
            | Self::ProposalAccepted { match_id, .. }
            | Self::ProposalDeclined { match_id, .. }
            | Self::ScoreReported { match_id, .. }
            | Self::ScoreDisputed { match_id, .. }
            | Self::MatchDecided { match_id }
            | Self::Challenged { match_id, .. }
            | Self::RequestFilled { match_id }
            | Self::MatchCancelled { match_id, .. } => match_id,
        }
    }
}

/// Queues `event` for each of `recipients`; the job loop sends them once the calling
/// transaction commits (and not at all if it rolls back).
pub async fn tell(
    tx: &mut TenantTx,
    recipients: impl IntoIterator<Item = Uuid>,
    event: Event,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let community_id = tx.community_id();
    for player_id in recipients {
        let job = Job::Notify {
            community_id,
            player_id,
            event,
        };
        jobs::enqueue(&mut **tx, job, now).await?;
    }
    Ok(())
}

/// The players on the other side of the match from `player`.
pub fn other_side(found: &MatchRow, player: Uuid) -> Vec<Uuid> {
    match found.side_of(player) {
        Some(Side::A) => found.side_b_players.clone(),
        Some(Side::B) => found.side_a_players.clone(),
        None => Vec::new(),
    }
}

/// Everyone playing in the match except `player` (who caused the news).
pub fn everyone_but(found: &MatchRow, player: Uuid) -> Vec<Uuid> {
    found
        .side_a_players
        .iter()
        .chain(&found.side_b_players)
        .copied()
        .filter(|&id| id != player)
        .collect()
}

/// What a notification says and where tapping it leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Bold first line.
    pub title: String,
    /// The line under it.
    pub body: String,
    /// The app path to open (`/matches/<id>`).
    pub url: String,
}

#[derive(FromRow)]
struct Recipient {
    email: String,
    display_name: String,
    email_verified: bool,
}

#[derive(FromRow)]
struct Prefs {
    match_updates: bool,
    league_updates: bool,
    reminders: bool,
}

/// Sends one notification (the `notify` job). Nothing is sent to a player who left or was
/// banned, who turned the category off, or for whom the news no longer holds.
pub async fn send(
    state: &AppState,
    community_id: Uuid,
    player_id: Uuid,
    event: Event,
) -> anyhow::Result<()> {
    let mut tx = TenantTx::begin(&state.db, community_id).await?;
    let recipient: Option<Recipient> = sqlx::query_as(
        "SELECT u.email, p.display_name, u.email_verified_at IS NOT NULL AS email_verified
         FROM players p JOIN users u ON u.id = p.user_id
         WHERE p.community_id = $1 AND p.id = $2 AND p.status = 'active'",
    )
    .bind(community_id)
    .bind(player_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(recipient) = recipient else {
        return Ok(());
    };
    if !wants(&mut tx, player_id, event.category()).await? {
        return Ok(());
    }
    let Some(note) = compose(&mut tx, player_id, event).await? else {
        return Ok(());
    };
    let tokens: Vec<String> = sqlx::query_scalar(
        "SELECT expo_push_token FROM device_tokens
         WHERE community_id = $1 AND player_id = $2 ORDER BY last_seen_at DESC",
    )
    .bind(community_id)
    .bind(player_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    if tokens.is_empty() {
        if event.emails_without_devices() && recipient.email_verified {
            email(state, community_id, &recipient, event, &note).await?;
        }
        return Ok(());
    }
    push(state, community_id, tokens, &note).await
}

async fn wants(tx: &mut TenantTx, player: Uuid, category: Category) -> anyhow::Result<bool> {
    let prefs: Option<Prefs> = sqlx::query_as(
        "SELECT match_updates, league_updates, reminders FROM notification_prefs
         WHERE community_id = $1 AND player_id = $2",
    )
    .bind(tx.community_id())
    .bind(player)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(prefs.is_none_or(|prefs| match category {
        Category::MatchUpdates => prefs.match_updates,
        Category::LeagueUpdates => prefs.league_updates,
        Category::Reminders => prefs.reminders,
    }))
}

async fn push(
    state: &AppState,
    community_id: Uuid,
    tokens: Vec<String>,
    note: &Note,
) -> anyhow::Result<()> {
    let messages: Vec<PushMessage> = tokens
        .into_iter()
        .map(|to| PushMessage {
            to,
            title: note.title.clone(),
            body: note.body.clone(),
            data: json!({ "url": note.url }),
        })
        .collect();
    let deliveries = state.pusher.send(&messages).await?;
    let gone: Vec<&str> = messages
        .iter()
        .zip(&deliveries)
        .filter(|(_, delivery)| **delivery == Delivery::Unregistered)
        .map(|(message, _)| message.to.as_str())
        .collect();
    if !gone.is_empty() {
        let mut tx = TenantTx::begin(&state.db, community_id).await?;
        let _ = sqlx::query(
            "DELETE FROM device_tokens WHERE community_id = $1 AND expo_push_token = ANY($2)",
        )
        .bind(community_id)
        .bind(&gone)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        tracing::info!(count = gone.len(), "forgot unregistered push tokens");
    }
    Ok(())
}

async fn email(
    state: &AppState,
    community_id: Uuid,
    recipient: &Recipient,
    event: Event,
    note: &Note,
) -> anyhow::Result<()> {
    let Some(tenant) = Tenant::load(&state.db, community_id).await? else {
        return Ok(());
    };
    let host = tenant
        .custom_domain
        .clone()
        .unwrap_or_else(|| format!("{}.{}", tenant.slug, state.config.base_domain));
    let deadline = match event {
        Event::ScoreReported { .. } => format!(
            " If nobody answers, it confirms itself after {} days.",
            CommunitySettings::of(&tenant).confirm_window_days
        ),
        _ => String::new(),
    };
    let text = format!(
        "Hi {},\n\n{}. {}{deadline}\n\nhttps://{host}{}\n\n{}",
        recipient.display_name, note.title, note.body, note.url, tenant.name
    );
    let message = Email {
        to: recipient.email.clone(),
        subject: format!("{}: {}", tenant.name, note.title),
        text,
    };
    state.mailer.send(&message).await
}

/// Writes the notification for `player` from the match as it is now, or `None` when the news
/// no longer holds (the score was answered, the player is no longer in the match).
async fn compose(tx: &mut TenantTx, player: Uuid, event: Event) -> anyhow::Result<Option<Note>> {
    let found = matches::load(tx, event.match_id(), false).await?;
    let Some(side) = found.side_of(player) else {
        return Ok(None);
    };
    let names = Names::load(
        tx,
        found
            .side_a_players
            .iter()
            .chain(&found.side_b_players)
            .copied(),
    )
    .await?;
    let them = names.joined(found.players(other(side)));
    let url = format!("/matches/{}", found.id);
    let note = |title: String, body: String| {
        Some(Note {
            title,
            body,
            url: url.clone(),
        })
    };
    let status = found.status();
    Ok(match event {
        Event::ProposalReceived { by, .. } => note(
            format!("{} proposed a time", names.name(by)),
            "Accept it, or suggest a time that suits you better.".to_owned(),
        ),
        Event::ProposalAccepted { by, .. } => note(
            format!("{} accepted your time", names.name(by)),
            format!("Your match against {them} is on."),
        ),
        Event::ProposalDeclined { by, .. } => note(
            format!("{} declined your time", names.name(by)),
            "Propose another time that suits you both.".to_owned(),
        ),
        Event::ScoreReported { by, .. } => match (&found.score, status) {
            (Some(score), MatchStatus::Reported) => note(
                format!("{} reported a score", names.name(by)),
                format!(
                    "{} against {them}: confirm it or dispute it.",
                    score_line(score, side)
                ),
            ),
            _ => None,
        },
        Event::ScoreDisputed { by, .. } if status == MatchStatus::Disputed => note(
            format!("{} disputed your score", names.name(by)),
            "A club admin will decide the result.".to_owned(),
        ),
        Event::ScoreDisputed { .. } => None,
        Event::MatchDecided { .. } => decided(&found, side, &them).map(|(title, body)| Note {
            title,
            body,
            url: url.clone(),
        }),
        Event::Challenged { by, .. } => note(
            format!("{} challenged you", names.name(by)),
            "Propose a time, or answer theirs.".to_owned(),
        ),
        Event::RequestFilled { .. } => note(
            "Your match request is full".to_owned(),
            format!("You play {them}. Agree a time in the app."),
        ),
        Event::MatchCancelled { by, .. } => note(
            format!("{} cancelled your match", names.name(by)),
            found
                .resolution_note
                .clone()
                .unwrap_or_else(|| "It won’t be played.".to_owned()),
        ),
    })
}

/// The words for an admin's decision, by where it left the match.
fn decided(found: &MatchRow, side: Side, them: &str) -> Option<(String, String)> {
    let ruling = found
        .resolution_note
        .as_ref()
        .map(|note| format!(" “{note}”"))
        .unwrap_or_default();
    let won = found.winner_side.map(Side::from) == Some(side);
    match found.status() {
        MatchStatus::Resolved => {
            let score = found
                .score
                .as_ref()
                .map(|score| score_line(score, side))
                .unwrap_or_default();
            Some((
                "Your disputed match was decided".to_owned(),
                format!("{score} against {them}.{ruling}"),
            ))
        }
        MatchStatus::Walkover => Some((
            if won {
                "You were awarded a walkover"
            } else {
                "Your match was awarded to your opponents"
            }
            .to_owned(),
            format!("Against {them}.{ruling}"),
        )),
        MatchStatus::Scheduled | MatchStatus::Proposed => Some((
            "Your match will be replayed".to_owned(),
            format!("A club admin ordered a replay against {them}.{ruling}"),
        )),
        MatchStatus::Cancelled => Some((
            "Your match was cancelled".to_owned(),
            format!("A club admin cancelled your match against {them}.{ruling}"),
        )),
        MatchStatus::Reported | MatchStatus::Confirmed | MatchStatus::Disputed => None,
    }
}

const fn other(side: Side) -> Side {
    match side {
        Side::A => Side::B,
        Side::B => Side::A,
    }
}

/// "6–4 3–6 [10–7]" read from `side`.
fn score_line(score: &Score, side: Side) -> String {
    score
        .sets
        .iter()
        .map(|set| {
            let (mine, theirs) = match side {
                Side::A => (set.a, set.b),
                Side::B => (set.b, set.a),
            };
            if set.match_tiebreak {
                format!("[{mine}–{theirs}]")
            } else {
                format!("{mine}–{theirs}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use courtpit_domain::SetScore;

    use super::*;

    #[test]
    fn scores_read_from_the_recipients_side() {
        let score = Score {
            sets: vec![
                SetScore {
                    a: 6,
                    b: 4,
                    match_tiebreak: false,
                },
                SetScore {
                    a: 3,
                    b: 6,
                    match_tiebreak: false,
                },
                SetScore {
                    a: 10,
                    b: 7,
                    match_tiebreak: true,
                },
            ],
        };
        assert_eq!(score_line(&score, Side::A), "6–4 3–6 [10–7]");
        assert_eq!(score_line(&score, Side::B), "4–6 6–3 [7–10]");
    }

    #[test]
    fn events_keep_their_wire_shape() {
        let event = Event::ScoreReported {
            match_id: Uuid::nil(),
            by: Uuid::nil(),
        };
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["type"], "score_reported");
        assert_eq!(serde_json::from_value::<Event>(value).unwrap(), event);
        assert!(event.emails_without_devices());
        assert_eq!(event.category(), Category::MatchUpdates);
    }
}
