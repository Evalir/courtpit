//! Match persistence: rows, Postgres enum mirrors of the domain types, and the glue that runs
//! domain state transitions against a locked row.
//!
//! Handlers live in `api::matches`; everything here takes a [`TenantTx`] so it is scoped.

use chrono::{DateTime, Utc};
use courtpit_domain::{
    Actor, Discipline, Event, MatchFormat, MatchKind, MatchState, MatchStatus, Score, Side,
    TransitionError, side_of,
};
use serde::Serialize;
use sqlx::{FromRow, types::Json};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{ApiError, TenantTx, auth::CurrentPlayer};

pub mod results;

/// Postgres `discipline`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "discipline", rename_all = "lowercase")]
pub enum DbDiscipline {
    /// One player per side.
    Singles,
    /// Two players per side of any gender mix.
    Doubles,
    /// Two players per side, one of each gender.
    Mixed,
}

impl From<Discipline> for DbDiscipline {
    fn from(discipline: Discipline) -> Self {
        match discipline {
            Discipline::Singles => Self::Singles,
            Discipline::Doubles => Self::Doubles,
            Discipline::Mixed => Self::Mixed,
        }
    }
}

impl From<DbDiscipline> for Discipline {
    fn from(discipline: DbDiscipline) -> Self {
        match discipline {
            DbDiscipline::Singles => Self::Singles,
            DbDiscipline::Doubles => Self::Doubles,
            DbDiscipline::Mixed => Self::Mixed,
        }
    }
}

/// Postgres `match_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "match_status", rename_all = "lowercase")]
pub enum DbMatchStatus {
    /// Players are still agreeing on a time.
    Proposed,
    /// A time is agreed; the match is awaiting play.
    Scheduled,
    /// A score was reported and awaits confirmation.
    Reported,
    /// The opponent confirmed the reported score.
    Confirmed,
    /// The opponent disputed the score; an admin must decide.
    Disputed,
    /// An admin decided the result.
    Resolved,
    /// Decided by forfeit without play.
    Walkover,
    /// Called off before being played.
    Cancelled,
}

impl From<MatchStatus> for DbMatchStatus {
    fn from(status: MatchStatus) -> Self {
        match status {
            MatchStatus::Proposed => Self::Proposed,
            MatchStatus::Scheduled => Self::Scheduled,
            MatchStatus::Reported => Self::Reported,
            MatchStatus::Confirmed => Self::Confirmed,
            MatchStatus::Disputed => Self::Disputed,
            MatchStatus::Resolved => Self::Resolved,
            MatchStatus::Walkover => Self::Walkover,
            MatchStatus::Cancelled => Self::Cancelled,
        }
    }
}

impl From<DbMatchStatus> for MatchStatus {
    fn from(status: DbMatchStatus) -> Self {
        match status {
            DbMatchStatus::Proposed => Self::Proposed,
            DbMatchStatus::Scheduled => Self::Scheduled,
            DbMatchStatus::Reported => Self::Reported,
            DbMatchStatus::Confirmed => Self::Confirmed,
            DbMatchStatus::Disputed => Self::Disputed,
            DbMatchStatus::Resolved => Self::Resolved,
            DbMatchStatus::Walkover => Self::Walkover,
            DbMatchStatus::Cancelled => Self::Cancelled,
        }
    }
}

/// Postgres `match_side`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "match_side", rename_all = "lowercase")]
pub enum DbSide {
    /// Side A, the proposer's side.
    #[sqlx(rename = "a")]
    SideA,
    /// Side B, the opponents.
    #[sqlx(rename = "b")]
    SideB,
}

impl From<Side> for DbSide {
    fn from(side: Side) -> Self {
        match side {
            Side::A => Self::SideA,
            Side::B => Self::SideB,
        }
    }
}

impl From<DbSide> for Side {
    fn from(side: DbSide) -> Self {
        match side {
            DbSide::SideA => Self::A,
            DbSide::SideB => Self::B,
        }
    }
}

/// Columns for [`MatchRow`].
pub const MATCH_COLUMNS: &str = "id, discipline, league_id, division_id, tournament_id, round, \
    side_a_players, side_b_players, status, scheduled_at, location, match_format, score, \
    winner_side, reported_by, reported_at, confirm_deadline_at, disputed_by, dispute_note, \
    resolved_by, resolution_note, created_by, created_at";

/// A `matches` row.
#[derive(Debug, Clone, FromRow)]
pub struct MatchRow {
    /// Match id.
    pub id: Uuid,
    /// Singles, doubles or mixed.
    pub discipline: DbDiscipline,
    /// League the match belongs to, if any.
    pub league_id: Option<Uuid>,
    /// Division the match belongs to, if any.
    pub division_id: Option<Uuid>,
    /// Tournament the match belongs to, if any.
    pub tournament_id: Option<Uuid>,
    /// Round within the league or tournament, if any.
    pub round: Option<i32>,
    /// Player ids on side A.
    pub side_a_players: Vec<Uuid>,
    /// Player ids on side B.
    pub side_b_players: Vec<Uuid>,
    /// Current lifecycle status.
    pub status: DbMatchStatus,
    /// Agreed start time, if scheduled.
    pub scheduled_at: Option<DateTime<Utc>>,
    /// Free-text place of play.
    pub location: Option<String>,
    /// Format the match is played in.
    pub match_format: Json<MatchFormat>,
    /// Reported score, if any.
    pub score: Option<Json<Score>>,
    /// Winning side, once decided.
    pub winner_side: Option<DbSide>,
    /// Player who reported the score.
    pub reported_by: Option<Uuid>,
    /// When the score was reported.
    pub reported_at: Option<DateTime<Utc>>,
    /// When the report auto-confirms if left unanswered.
    pub confirm_deadline_at: Option<DateTime<Utc>>,
    /// Player who disputed the reported score.
    pub disputed_by: Option<Uuid>,
    /// Why the reported score was disputed.
    pub dispute_note: Option<String>,
    /// Player who resolved or cancelled the match.
    pub resolved_by: Option<Uuid>,
    /// Note left by the resolver.
    pub resolution_note: Option<String>,
    /// Player who created the match.
    pub created_by: Option<Uuid>,
    /// When the row was created.
    pub created_at: DateTime<Utc>,
}

impl MatchRow {
    /// Domain status.
    pub fn status(&self) -> MatchStatus {
        self.status.into()
    }

    /// Friendly matches have no league or tournament.
    pub const fn kind(&self) -> MatchKind {
        if self.league_id.is_some() || self.tournament_id.is_some() {
            MatchKind::Competitive
        } else {
            MatchKind::Friendly
        }
    }

    /// Which side `player` is on, if any.
    pub fn side_of(&self, player: Uuid) -> Option<Side> {
        side_of(&player, &self.side_a_players, &self.side_b_players)
    }

    /// Players on `side`.
    pub fn players(&self, side: Side) -> &[Uuid] {
        match side {
            Side::A => &self.side_a_players,
            Side::B => &self.side_b_players,
        }
    }

    /// Whether `player` plays in this match.
    pub fn involves(&self, player: Uuid) -> bool {
        self.side_of(player).is_some()
    }

    /// The domain state machine's view of this row.
    pub fn state(&self) -> Result<MatchState, ApiError> {
        let reported_by = self.reported_by.and_then(|reporter| self.side_of(reporter));
        MatchState::from_parts(self.status(), reported_by).ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "match {} is reported but its reporter plays on neither side",
                self.id
            ))
        })
    }

    /// Steps the domain state machine with `event` by `actor` and returns the new status.
    pub fn transition(&self, actor: Actor, event: Event) -> Result<MatchStatus, ApiError> {
        self.state()?
            .step(self.kind(), actor, event)
            .map(MatchState::status)
            .map_err(transition_error)
    }
}

/// Maps a refused transition to an API error.
pub fn transition_error(err: TransitionError) -> ApiError {
    match err {
        TransitionError::Forbidden(msg) => ApiError::forbidden(msg),
        other => ApiError::conflict(other.to_string()),
    }
}

/// Loads a match of the transaction's community, optionally locking it for update.
pub async fn load(tx: &mut TenantTx, id: Uuid, lock: bool) -> Result<MatchRow, ApiError> {
    let sql = format!(
        "SELECT {MATCH_COLUMNS} FROM matches WHERE community_id = $1 AND id = $2{}",
        if lock { " FOR UPDATE" } else { "" }
    );
    sqlx::query_as(&sql)
        .bind(tx.community_id())
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(ApiError::NotFound("match"))
}

/// The player acting on their own behalf: they must play in the match.
pub fn player_actor(found: &MatchRow, player: &CurrentPlayer) -> Result<Actor, ApiError> {
    found
        .side_of(player.id)
        .map(Actor::Player)
        .ok_or_else(|| ApiError::forbidden("you are not playing in this match"))
}

/// The caller as a player if they play in the match, otherwise as an admin if they are one.
pub fn player_or_admin_actor(found: &MatchRow, player: &CurrentPlayer) -> Result<Actor, ApiError> {
    match found.side_of(player.id) {
        Some(side) => Ok(Actor::Player(side)),
        None if player.role.is_admin() => Ok(Actor::Admin),
        None => Err(ApiError::forbidden("you are not playing in this match")),
    }
}

/// Admin acting as referee. Admins may not referee matches they play in (owners may, so a
/// one-admin community is never stuck).
pub fn admin_actor(found: &MatchRow, player: &CurrentPlayer) -> Result<Actor, ApiError> {
    player.require_admin()?;
    if found.involves(player.id) && player.role != crate::models::PlayerRole::Owner {
        return Err(ApiError::forbidden(
            "you play in this match; another admin or the owner must decide it",
        ));
    }
    Ok(Actor::Admin)
}

/// A match to insert.
#[derive(Debug, Clone)]
pub struct NewMatch<'a> {
    /// Singles or doubles.
    pub discipline: Discipline,
    /// Players on side A.
    pub side_a: &'a [Uuid],
    /// Players on side B.
    pub side_b: &'a [Uuid],
    /// Scoring format of the match.
    pub format: MatchFormat,
    /// Player who created the match, if any.
    pub created_by: Option<Uuid>,
    /// Set for league matches; `None` makes a friendly.
    pub league: Option<LeagueSlot>,
}

/// Where a league match sits in its league's schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeagueSlot {
    /// The league the match belongs to.
    pub league_id: Uuid,
    /// The division (box) within the league.
    pub division_id: Uuid,
    /// The 1-based round of the schedule.
    pub round: i32,
}

/// Inserts a `proposed` match into the transaction's community; returns its id. Callers
/// validate the players first.
pub async fn insert(tx: &mut TenantTx, new: &NewMatch<'_>) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    let _ = sqlx::query(
        "INSERT INTO matches (id, community_id, discipline, side_a_players, side_b_players,
                              match_format, created_by, league_id, division_id, round)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(id)
    .bind(tx.community_id())
    .bind(DbDiscipline::from(new.discipline))
    .bind(new.side_a)
    .bind(new.side_b)
    .bind(Json(new.format))
    .bind(new.created_by)
    .bind(new.league.map(|slot| slot.league_id))
    .bind(new.league.map(|slot| slot.division_id))
    .bind(new.league.map(|slot| slot.round))
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

/// Members can see matches they play in and every competitive match; admins see all.
pub fn visible_to(found: &MatchRow, player: &CurrentPlayer) -> bool {
    player.role.is_admin() || found.involves(player.id) || found.kind() == MatchKind::Competitive
}

/// Sets the status (and `updated_at`) of a locked match.
pub async fn set_status(
    tx: &mut TenantTx,
    id: Uuid,
    status: MatchStatus,
) -> Result<(), sqlx::Error> {
    let _ = sqlx::query(
        "UPDATE matches SET status = $3, updated_at = now() WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(id)
    .bind(DbMatchStatus::from(status))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Postgres `proposal_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema, sqlx::Type)]
#[sqlx(type_name = "proposal_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum ProposalStatus {
    /// Awaiting a response from the other side.
    Open,
    /// Accepted; the match was scheduled at the proposed time.
    Accepted,
    /// Declined by the other side.
    Declined,
    /// Replaced by a newer proposal.
    Superseded,
}

/// A time-and-place proposal for a match.
#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct Proposal {
    /// Proposal id.
    pub id: Uuid,
    /// Player who made the proposal.
    pub proposed_by: Uuid,
    /// Proposed start time.
    pub proposed_time: DateTime<Utc>,
    /// Proposed place, if any.
    pub location: Option<String>,
    /// Where the proposal stands.
    pub status: ProposalStatus,
    /// When the proposal was made.
    pub created_at: DateTime<Utc>,
}

/// All proposals of a match, oldest first.
pub async fn proposals(tx: &mut TenantTx, match_id: Uuid) -> Result<Vec<Proposal>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, proposed_by, proposed_time, location, status, created_at
         FROM match_proposals WHERE community_id = $1 AND match_id = $2 ORDER BY created_at, id",
    )
    .bind(tx.community_id())
    .bind(match_id)
    .fetch_all(&mut **tx)
    .await
}

/// Records a new open proposal, superseding any other open one.
pub async fn insert_proposal(
    tx: &mut TenantTx,
    match_id: Uuid,
    proposed_by: Uuid,
    time: DateTime<Utc>,
    location: Option<&str>,
) -> Result<Uuid, sqlx::Error> {
    let _ = sqlx::query(
        "UPDATE match_proposals SET status = 'superseded', updated_at = now()
         WHERE community_id = $1 AND match_id = $2 AND status = 'open'",
    )
    .bind(tx.community_id())
    .bind(match_id)
    .execute(&mut **tx)
    .await?;
    let id = Uuid::now_v7();
    let _ = sqlx::query(
        "INSERT INTO match_proposals (id, community_id, match_id, proposed_by, proposed_time, location)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(tx.community_id())
    .bind(match_id)
    .bind(proposed_by)
    .bind(time)
    .bind(location)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

/// API view of a match.
#[derive(Debug, Serialize, ToSchema)]
pub struct MatchView {
    /// Match id.
    pub id: Uuid,
    /// Singles, doubles or mixed.
    pub discipline: Discipline,
    /// League the match belongs to, if any.
    pub league_id: Option<Uuid>,
    /// Division the match belongs to, if any.
    pub division_id: Option<Uuid>,
    /// Tournament the match belongs to, if any.
    pub tournament_id: Option<Uuid>,
    /// Round within the league or tournament, if any.
    pub round: Option<i32>,
    /// Player ids on side A.
    pub side_a: Vec<Uuid>,
    /// Player ids on side B.
    pub side_b: Vec<Uuid>,
    /// Current lifecycle status.
    pub status: MatchStatus,
    /// Agreed start time, if scheduled.
    pub scheduled_at: Option<DateTime<Utc>>,
    /// Free-text place of play.
    pub location: Option<String>,
    /// Format the match is played in.
    pub match_format: MatchFormat,
    /// Reported score, if any.
    pub score: Option<Score>,
    /// Winning side, once decided.
    pub winner_side: Option<Side>,
    /// Player who reported the score.
    pub reported_by: Option<Uuid>,
    /// When the score was reported.
    pub reported_at: Option<DateTime<Utc>>,
    /// Unanswered reports auto-confirm at this time.
    pub confirm_deadline_at: Option<DateTime<Utc>>,
    /// Player who disputed the reported score.
    pub disputed_by: Option<Uuid>,
    /// Why the reported score was disputed.
    pub dispute_note: Option<String>,
    /// Player who resolved or cancelled the match.
    pub resolved_by: Option<Uuid>,
    /// Note left by the resolver.
    pub resolution_note: Option<String>,
    /// When the row was created.
    pub created_at: DateTime<Utc>,
    /// Scheduling history; included on single-match responses only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposals: Option<Vec<Proposal>>,
}

impl From<MatchRow> for MatchView {
    fn from(row: MatchRow) -> Self {
        Self {
            id: row.id,
            discipline: row.discipline.into(),
            league_id: row.league_id,
            division_id: row.division_id,
            tournament_id: row.tournament_id,
            round: row.round,
            status: row.status.into(),
            side_a: row.side_a_players,
            side_b: row.side_b_players,
            scheduled_at: row.scheduled_at,
            location: row.location,
            match_format: row.match_format.0,
            score: row.score.map(|score| score.0),
            winner_side: row.winner_side.map(Into::into),
            reported_by: row.reported_by,
            reported_at: row.reported_at,
            confirm_deadline_at: row.confirm_deadline_at,
            disputed_by: row.disputed_by,
            dispute_note: row.dispute_note,
            resolved_by: row.resolved_by,
            resolution_note: row.resolution_note,
            created_at: row.created_at,
            proposals: None,
        }
    }
}

/// A match with its proposals, for single-match responses.
pub async fn view_with_proposals(
    tx: &mut TenantTx,
    match_row: MatchRow,
) -> Result<MatchView, ApiError> {
    let proposals = proposals(tx, match_row.id).await?;
    let mut view = MatchView::from(match_row);
    view.proposals = Some(proposals);
    Ok(view)
}
