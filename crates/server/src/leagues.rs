//! League rows, effective rules (format and scoring) and the shared loaders.

use chrono::{DateTime, Utc};
use courtpit_domain::{BoxSize, Discipline, MatchFormat, ScoringConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, types::Json};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{ApiError, Tenant, TenantTx, matches::DbDiscipline};

pub mod entries;

/// Lifecycle of a league season.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[sqlx(type_name = "league_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum LeagueStatus {
    /// Being set up; not yet public.
    Draft,
    /// Published and accepting registrations.
    Registration,
    /// Season in progress.
    Active,
    /// Season completed.
    Finished,
    /// Cancelled by an admin.
    Cancelled,
}

/// Columns for [`LeagueRow`].
pub const LEAGUE_COLUMNS: &str = "id, name, discipline, registration_opens_at, \
    registration_closes_at, starts_at, ends_at, status, published_at, match_format, \
    entry_fee_minor, scoring_overrides, box_min_size, box_max_size, previous_league_id, \
    created_at";

/// A `leagues` row.
#[derive(Debug, Clone, FromRow)]
pub struct LeagueRow {
    /// League id.
    pub id: Uuid,
    /// Display name.
    pub name: String,
    /// Racket sport the league is played in.
    pub discipline: DbDiscipline,
    /// When registration opens.
    pub registration_opens_at: DateTime<Utc>,
    /// When registration closes.
    pub registration_closes_at: DateTime<Utc>,
    /// When the season starts.
    pub starts_at: DateTime<Utc>,
    /// When the season ends.
    pub ends_at: DateTime<Utc>,
    /// Lifecycle status.
    pub status: LeagueStatus,
    /// When the league was published, if it has been.
    pub published_at: Option<DateTime<Utc>>,
    /// Match format override.
    pub match_format: Option<Json<MatchFormat>>,
    /// Entry fee in minor currency units (unused until payments ship).
    pub entry_fee_minor: Option<i32>,
    /// Scoring config overrides.
    pub scoring_overrides: Option<Json<Value>>,
    /// Minimum box size.
    pub box_min_size: i32,
    /// Maximum box size.
    pub box_max_size: i32,
    /// The preceding season, if any.
    pub previous_league_id: Option<Uuid>,
    /// When the row was created.
    pub created_at: DateTime<Utc>,
}

impl LeagueRow {
    /// The league's discipline.
    pub fn discipline(&self) -> Discipline {
        self.discipline.into()
    }

    /// Whether players can register (or change entries) at `now`.
    pub fn registration_open(&self, now: DateTime<Utc>) -> bool {
        self.status == LeagueStatus::Registration
            && now >= self.registration_opens_at
            && now < self.registration_closes_at
    }

    /// The league's format, or the community default.
    pub fn format(&self, tenant: &Tenant) -> Result<MatchFormat, ApiError> {
        self.match_format.as_ref().map_or_else(
            || crate::api::matches::community_format(tenant),
            |format| Ok(format.0),
        )
    }

    /// The community's scoring config with this league's overrides merged in.
    pub fn scoring(&self, tenant: &Tenant) -> Result<ScoringConfig, ApiError> {
        scoring_with(tenant, self.scoring_overrides.as_ref().map(|json| &json.0))
            .map_err(|err| ApiError::Internal(anyhow::anyhow!("league scoring config: {err}")))
    }

    /// Box size limits for placement.
    pub fn box_size(&self) -> BoxSize {
        BoxSize {
            min_size: usize::try_from(self.box_min_size).unwrap_or(6),
            max_size: usize::try_from(self.box_max_size).unwrap_or(8),
        }
    }
}

/// Recursively merges `over` into `base` (objects merge key by key; anything else replaces).
pub fn merge_json(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(base), Value::Object(over)) => {
            for (key, value) in over {
                merge_json(base.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
        (base, over) => *base = over.clone(),
    }
}

/// The community scoring config with optional overrides merged in.
pub fn scoring_with(
    tenant: &Tenant,
    overrides: Option<&Value>,
) -> Result<ScoringConfig, serde_json::Error> {
    let mut config = tenant.scoring_config.0.clone();
    if let Some(overrides_value) = overrides {
        merge_json(&mut config, overrides_value);
    }
    serde_json::from_value(config)
}

/// Loads a league of the transaction's community, optionally locking it. Locking the league
/// row serialises every registration change for that league.
pub async fn load(tx: &mut TenantTx, id: Uuid, lock: bool) -> Result<LeagueRow, ApiError> {
    let sql = format!(
        "SELECT {LEAGUE_COLUMNS} FROM leagues WHERE community_id = $1 AND id = $2{}",
        if lock { " FOR UPDATE" } else { "" }
    );
    sqlx::query_as(&sql)
        .bind(tx.community_id())
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(ApiError::NotFound("league"))
}

/// A league as the API shows it.
#[derive(Debug, Serialize, ToSchema)]
pub struct LeagueView {
    /// League id.
    pub id: Uuid,
    /// Display name.
    pub name: String,
    /// Racket sport the league is played in.
    pub discipline: Discipline,
    /// When registration opens.
    pub registration_opens_at: DateTime<Utc>,
    /// When registration closes.
    pub registration_closes_at: DateTime<Utc>,
    /// When the season starts.
    pub starts_at: DateTime<Utc>,
    /// When the season ends.
    pub ends_at: DateTime<Utc>,
    /// Lifecycle status.
    pub status: LeagueStatus,
    /// When the league was published, if it has been.
    pub published_at: Option<DateTime<Utc>>,
    /// The format league matches use (league override or community default).
    pub match_format: MatchFormat,
    /// Always null until in-app payments ship.
    pub entry_fee_minor: Option<i32>,
    /// Scoring config overrides.
    #[schema(value_type = Option<Object>)]
    pub scoring_overrides: Option<Value>,
    /// Minimum box size.
    pub box_min_size: i32,
    /// Maximum box size.
    pub box_max_size: i32,
    /// The preceding season, if any.
    pub previous_league_id: Option<Uuid>,
    /// When the row was created.
    pub created_at: DateTime<Utc>,
}

impl LeagueView {
    /// Builds the view, resolving the effective format.
    pub fn new(league: LeagueRow, tenant: &Tenant) -> Result<Self, ApiError> {
        Ok(Self {
            match_format: league.format(tenant)?,
            discipline: league.discipline(),
            id: league.id,
            name: league.name,
            registration_opens_at: league.registration_opens_at,
            registration_closes_at: league.registration_closes_at,
            starts_at: league.starts_at,
            ends_at: league.ends_at,
            status: league.status,
            published_at: league.published_at,
            entry_fee_minor: league.entry_fee_minor,
            scoring_overrides: league.scoring_overrides.map(|json| json.0),
            box_min_size: league.box_min_size,
            box_max_size: league.box_max_size,
            previous_league_id: league.previous_league_id,
            created_at: league.created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn merge_is_deep_and_replaces_leaves() {
        let mut base = json!({ "a": { "x": 1, "y": 2 }, "b": [1, 2], "c": "keep" });
        merge_json(&mut base, &json!({ "a": { "y": 3, "z": 4 }, "b": [9] }));
        assert_eq!(
            base,
            json!({ "a": { "x": 1, "y": 3, "z": 4 }, "b": [9], "c": "keep" })
        );
    }
}
