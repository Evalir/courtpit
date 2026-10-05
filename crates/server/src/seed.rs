//! `racquetcollective-server seed`: fills a dev/staging database with a demo community, so the
//! mobile app and reviewers have something to look at.
//!
//! Where a code path exists it is the real one: leagues are activated by
//! [`crate::leagues::lifecycle::advance`] (placement and round-robin schedule), results go
//! through the match state machine, [`crate::matches::results::record_report`] and
//! [`crate::rankings::on_match_result`], and rankings are rebuilt by the `refresh_rankings`
//! job's handler. Direct SQL only mirrors what the handlers write (users, players, leagues,
//! entries, match requests). Every time is relative to `state.clock.now()`.

use std::fmt;

use anyhow::Context;
use chrono::{DateTime, Duration, Utc};
use racquetcollective_domain::{Actor, Discipline, Event, Score, SetScore, Side};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sqlx::types::Json;
use uuid::Uuid;

use crate::{
    AppState, Tenant, TenantTx,
    communities::CommunitySettings,
    leagues::{entries::EntryStatus, lifecycle},
    matches::{self, DbDiscipline, DbMatchStatus, MatchRow, results},
    models::{Gender, PlayPref, PlayerRole},
    rankings,
};

const CLUB_NAME: &str = "Riverside Tennis Club";
#[rustfmt::skip]
const LOCATIONS: [&str; 4] = ["Riverside Courts", "Central Park Club", "Hillcrest Tennis Centre", "Harbour Sports Hall"];
#[rustfmt::skip]
const RACKETS: [&str; 4] = ["Babolat Pure Drive", "Wilson Blade 98", "Head Speed MP", "Yonex Ezone 100"];
const STRINGS: [&str; 3] = ["Luxilon ALU Power", "Babolat RPM Blast", "Wilson Natural Gut"];

/// The club's owner: name, UTR in tenths, gender.
const OWNER: (&str, i64, Gender) = ("Marcus Hale", 56, Gender::Male);
/// The members: name, UTR in tenths (3.0 to 8.0, ascending), gender.
#[rustfmt::skip]
const PLAYERS: [(&str, i64, Gender); 24] = [
    ("Ava Thompson", 30, Gender::Female), ("Liam Carter", 32, Gender::Male),
    ("Sofia Reyes", 35, Gender::Female), ("Noah Kim", 37, Gender::Male),
    ("Emma Larsson", 39, Gender::Female), ("Lucas Meyer", 41, Gender::Male),
    ("Mia Okafor", 43, Gender::Female), ("Ethan Brooks", 46, Gender::Male),
    ("Chloe Dubois", 48, Gender::Female), ("Oliver Singh", 50, Gender::Male),
    ("Zoe Nakamura", 52, Gender::Other), ("Mason Rossi", 54, Gender::Male),
    ("Isla Murphy", 56, Gender::Female), ("Henry Novak", 58, Gender::Male),
    ("Grace Adeyemi", 60, Gender::Female), ("Jack Sorensen", 62, Gender::Male),
    ("Lily Fernandez", 64, Gender::Female), ("Leo Petrov", 66, Gender::Undisclosed),
    ("Hannah Weiss", 69, Gender::Female), ("Daniel Costa", 71, Gender::Male),
    ("Priya Nair", 73, Gender::Female), ("Samuel Ortiz", 75, Gender::Male),
    ("Freya Lindqvist", 78, Gender::Female), ("Mateo Alvarez", 80, Gender::Male),
];
/// Winner-perspective scores `(games won, games lost, match tiebreak)`, all legal under the
/// community's default format.
#[rustfmt::skip]
const SCORES: [&[(u16, u16, bool)]; 6] = [
    &[(6, 2, false), (6, 3, false)], &[(6, 4, false), (6, 4, false)],
    &[(7, 5, false), (6, 4, false)], &[(7, 6, false), (6, 4, false)],
    &[(6, 3, false), (4, 6, false), (10, 7, true)], &[(4, 6, false), (6, 3, false), (10, 8, true)],
];

/// Fails when `env` (the value of `RACQUETCOLLECTIVE_ENV`) names production: seeding writes fake
/// people and results, so it must never run against a live database.
pub fn ensure_not_production(env: Option<&str>) -> anyhow::Result<()> {
    let name = env.unwrap_or_default().trim();
    let production = ["production", "prod"].iter().any(|label| name.eq_ignore_ascii_case(label));
    anyhow::ensure!(
        !production,
        "refusing to seed: RACQUETCOLLECTIVE_ENV is `{name}`; seed is for development and staging only"
    );
    Ok(())
}

/// What the demo community holds after [`seed`] (counted from the database, so a re-run
/// reports the same numbers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedSummary {
    /// The community's id.
    pub community_id: Uuid,
    /// The community's slug.
    pub slug: String,
    /// Email of the owner (sign in with an emailed code).
    pub owner_email: String,
    /// Row counts by label: players (owner included), leagues, league matches, open match
    /// requests and ranked players.
    pub counts: Vec<(&'static str, i64)>,
}

impl fmt::Display for SeedSummary {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            out,
            "seeded community {} ({})\n  owner: {}",
            self.slug, self.community_id, self.owner_email
        )?;
        self.counts.iter().try_for_each(|(what, count)| write!(out, "\n  {what}: {count}"))
    }
}

/// Fills the community `slug` with demo data: branding, an owner and 24 verified players, a
/// singles league mid-season (placed into boxes, with confirmed, reported and disputed
/// results), a doubles league in registration, two open match requests and the rankings.
///
/// Idempotent: the community is upserted by slug (name and branding refreshed), users by
/// email, players by (community, user) with their profile refreshed (roles only ever
/// promoted), a league is created only when none with its name exists (an existing one is
/// left exactly as it is) and a match request only when its creator has none at the same
/// location.
pub async fn seed(state: &AppState, slug: &str) -> anyhow::Result<SeedSummary> {
    let community_id: Uuid = sqlx::query_scalar(
        "INSERT INTO communities (id, slug, name, branding) VALUES ($1, $2, $3, $4)
         ON CONFLICT (slug) DO UPDATE
            SET name = excluded.name, branding = excluded.branding, updated_at = now()
         RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(slug.trim().to_ascii_lowercase())
    .bind(CLUB_NAME)
    .bind(Json(branding()))
    .fetch_one(&state.db)
    .await
    .context("upserting community (slug must be lowercase letters, digits and dashes)")?;
    let tenant =
        Tenant::load(&state.db, community_id).await?.context("community vanished after upsert")?;
    let (owner, players) = upsert_members(state, &tenant).await?;
    let ids = |keep: fn(PlayPref) -> bool| -> Vec<Uuid> {
        players.iter().filter(|(_, pref)| keep(*pref)).map(|(id, _)| *id).collect()
    };
    let doubles = ids(|pref| pref != PlayPref::Singles);
    let singles: Vec<Uuid> =
        ids(|pref| pref != PlayPref::Doubles).into_iter().skip(2).take(14).collect();
    anyhow::ensure!(singles.len() == 14 && doubles.len() >= 10, "too few demo players");

    let entries: Vec<Entry> = singles.iter().map(|&id| (vec![id], None)).collect();
    if let Some(league) = new_league(state, &tenant, owner, &SINGLES, &entries).await? {
        record_results(state, &tenant, league).await?;
    }
    let mut entries: Vec<Entry> =
        doubles.chunks(2).take(3).map(|pair| (pair.to_vec(), None)).collect();
    entries.push((vec![doubles[6]], Some(doubles[7])));
    let _ = new_league(state, &tenant, owner, &DOUBLES, &entries).await?;
    open_match_requests(state, &tenant, [singles[4], doubles[8], doubles[9]]).await?;
    rankings::refresh_job(state, community_id).await?;
    summarize(state, &tenant).await
}

/// The club's branding: every token the clients read.
fn branding() -> Value {
    json!({
        "display_name": CLUB_NAME,
        "logo_url": "https://example.com/demo/riverside-logo.png",
        "colors": { "primary": "#0b6e4f", "secondary": "#f4b942", "background": "#f7f9f4",
                    "surface": "#ffffff", "text": "#1b2a22" },
        "typography": "inter",
        "feature_flags": { "doubles": true, "mixed_doubles": true, "match_requests": true },
    })
}

fn email_of(name: &str) -> String {
    format!("{}@example.com", name.to_ascii_lowercase().replace(' ', "."))
}

/// Upserts the owner and the 24 players with verified emails. Profiles vary with a member's
/// position (some phones and socials shared, some private, gaps in the gear) so the directory
/// looks lived in. Returns the owner's player id and the players' ids with their preference.
async fn upsert_members(
    state: &AppState,
    tenant: &Tenant,
) -> anyhow::Result<(Uuid, Vec<(Uuid, PlayPref)>)> {
    let mut tx = tenant.begin(&state.db).await?;
    let mut members = Vec::new();
    for (index, (name, utr_tenths, gender)) in std::iter::once(OWNER).chain(PLAYERS).enumerate() {
        let email = email_of(name);
        let user: Uuid = sqlx::query_scalar(
            "INSERT INTO users (id, email, email_verified_at) VALUES ($1, $2, $3)
             ON CONFLICT (lower(email)) DO UPDATE
                SET email_verified_at = coalesce(users.email_verified_at, excluded.email_verified_at)
             RETURNING id",
        )
        .bind(Uuid::now_v7())
        .bind(&email)
        .bind(state.clock.now())
        .fetch_one(&state.db)
        .await
        .context("upserting user")?;
        let pref = [PlayPref::Singles, PlayPref::Doubles, PlayPref::Any][index % 3];
        let mut locations = vec![LOCATIONS[index % 4]];
        if index.is_multiple_of(3) {
            locations.push(LOCATIONS[(index + 1) % 4]);
        }
        let strings = STRINGS.get(index % 4).copied();
        let handle = (index % 3 != 2).then(|| name.to_ascii_lowercase().replace(' ', "_"));
        let socials = handle
            .as_ref()
            .map_or_else(|| json!({}), |tag| json!({ "instagram": format!("@{tag}") }));
        let player: Uuid = sqlx::query_scalar(
            "INSERT INTO players (id, community_id, user_id, display_name, utr, gender, phone,
                phone_visible, socials, socials_visible, racket, strings, tension_kg, play_pref,
                preferred_locations, role)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
             ON CONFLICT (community_id, user_id) DO UPDATE SET
                display_name = excluded.display_name, utr = excluded.utr,
                gender = excluded.gender, phone = excluded.phone,
                phone_visible = excluded.phone_visible, socials = excluded.socials,
                socials_visible = excluded.socials_visible, racket = excluded.racket,
                strings = excluded.strings, tension_kg = excluded.tension_kg,
                play_pref = excluded.play_pref,
                preferred_locations = excluded.preferred_locations,
                role = greatest(players.role, excluded.role), updated_at = now()
             RETURNING id",
        )
        .bind(Uuid::now_v7())
        .bind(tenant.id())
        .bind(user)
        .bind(name)
        .bind(Decimal::new(utr_tenths, 1))
        .bind(gender)
        .bind(index.is_multiple_of(2).then(|| format!("+1 555 01{index:02}")))
        .bind(index.is_multiple_of(4))
        .bind(Json(socials))
        .bind(handle.is_some() && index % 2 == 1)
        .bind(RACKETS.get(index % 5).copied())
        .bind(strings)
        .bind(strings.map(|_| Decimal::new(220, 1) + Decimal::new(5, 1) * Decimal::from(index % 5)))
        .bind(pref)
        .bind(Json(locations))
        .bind(if index == 0 { PlayerRole::Owner } else { PlayerRole::Player })
        .fetch_one(&mut *tx)
        .await
        .context("upserting player")?;
        members.push((player, pref));
    }
    tx.commit().await?;
    let mut members = members.into_iter();
    let (owner, _) = members.next().context("no owner")?;
    Ok((owner, members.collect()))
}

/// A league's name, discipline and its dates as (days from now, hour of day UTC): registration
/// opens, registration closes, the season starts, the season ends.
struct LeagueSpec {
    name: &'static str,
    discipline: Discipline,
    dates: [(i64, u32); 4],
}

/// Mid-season: started four weeks ago, ends in eight.
const SINGLES: LeagueSpec = LeagueSpec {
    name: "Autumn Singles League",
    discipline: Discipline::Singles,
    dates: [(-45, 0), (-31, 23), (-28, 0), (56, 23)],
};
/// Registration open now, the season starts in two weeks.
const DOUBLES: LeagueSpec = LeagueSpec {
    name: "Winter Doubles League",
    discipline: Discipline::Doubles,
    dates: [(-7, 0), (12, 23), (14, 0), (98, 23)],
};

/// A league entry to insert: its players (the first one registers it) and, for a
/// `pending_partner` entry, the partner invited to complete it.
type Entry = (Vec<Uuid>, Option<Uuid>);

/// `days` from now at `hour` o'clock UTC.
fn slot(now: DateTime<Utc>, days: i64, hour: u32) -> DateTime<Utc> {
    (now + Duration::days(days))
        .date_naive()
        .and_hms_opt(hour, 0, 0)
        .map_or(now, |naive| naive.and_utc())
}

/// Creates a published league with its entries unless one with this name exists, then lets
/// the lifecycle handler apply every due step (open registration, place boxes, schedule).
/// Returns the new league's id.
async fn new_league(
    state: &AppState,
    tenant: &Tenant,
    owner: Uuid,
    spec: &LeagueSpec,
    entries: &[Entry],
) -> anyhow::Result<Option<Uuid>> {
    let mut tx = tenant.begin(&state.db).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM leagues WHERE community_id = $1 AND name = $2)",
    )
    .bind(tenant.id())
    .bind(spec.name)
    .fetch_one(&mut *tx)
    .await?;
    if exists {
        return Ok(None);
    }
    let now = state.clock.now();
    let [opens, closes, starts, ends] = spec.dates.map(|(days, hour)| slot(now, days, hour));
    let id = Uuid::now_v7();
    let _ = sqlx::query(
        "INSERT INTO leagues (id, community_id, name, discipline, registration_opens_at,
            registration_closes_at, starts_at, ends_at, published_at, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(id)
    .bind(tenant.id())
    .bind(spec.name)
    .bind(DbDiscipline::from(spec.discipline))
    .bind(opens)
    .bind(closes)
    .bind(starts)
    .bind(ends)
    .bind(opens - Duration::days(1))
    .bind(owner)
    .execute(&mut *tx)
    .await?;
    for (players, invited) in entries {
        let status =
            if invited.is_some() { EntryStatus::PendingPartner } else { EntryStatus::Confirmed };
        let _ = sqlx::query(
            "INSERT INTO league_entries (id, community_id, league_id, player_ids, created_by,
                status, invited_partner_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(Uuid::now_v7())
        .bind(tenant.id())
        .bind(id)
        .bind(players)
        .bind(players.first())
        .bind(status)
        .bind(invited)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    lifecycle::advance(state, tenant.id(), id).await?;
    Ok(Some(id))
}

/// What happens to a league match in the demo season.
#[derive(Debug, Clone, Copy)]
enum Fate {
    Confirmed,
    Reported,
    Disputed,
    Scheduled,
}

/// Rounds 1 and 2 are done, round 3 is in flight: the first match of box 1 awaits
/// confirmation, the first of box 2 is disputed and the rest are booked for the coming days.
const fn fate_of(tier: i32, round: i32, nth: i64) -> Option<Fate> {
    match (round, tier, nth) {
        (1 | 2, ..) => Some(Fate::Confirmed),
        (3, 1, 0) => Some(Fate::Reported),
        (3, 2, 0) => Some(Fate::Disputed),
        (3, ..) => Some(Fate::Scheduled),
        _ => None,
    }
}

/// Plays the season so far, in one transaction: schedules and settles the matches
/// [`fate_of`] picks. The side with the higher combined UTR is the favourite.
async fn record_results(state: &AppState, tenant: &Tenant, league: Uuid) -> anyhow::Result<()> {
    let when = (
        state.clock.now(),
        Duration::days(CommunitySettings::of(tenant).confirm_window_days.into()),
    );
    let mut tx = tenant.begin(&state.db).await?;
    let rows: Vec<(Uuid, i32, i32, i64, bool)> = sqlx::query_as(
        "SELECT m.id, d.tier, m.round,
            row_number() OVER (PARTITION BY d.tier, m.round ORDER BY m.id) - 1,
            (SELECT coalesce(sum(p.utr), 0) FROM players p
              WHERE p.community_id = m.community_id AND p.id = ANY(m.side_a_players))
            >= (SELECT coalesce(sum(p.utr), 0) FROM players p
                 WHERE p.community_id = m.community_id AND p.id = ANY(m.side_b_players))
         FROM matches m
         JOIN league_divisions d ON d.community_id = m.community_id AND d.id = m.division_id
         WHERE m.community_id = $1 AND m.league_id = $2 ORDER BY d.tier, m.round, m.id",
    )
    .bind(tenant.id())
    .bind(league)
    .fetch_all(&mut *tx)
    .await?;
    for (variant, (id, tier, round, nth, a_favoured)) in rows.into_iter().enumerate() {
        let Some(fate) = fate_of(tier, round, nth) else {
            continue;
        };
        let row = matches::load(&mut tx, id, true).await?;
        let favourite = if a_favoured { Side::A } else { Side::B };
        play(&mut tx, when, &row, (fate, favourite), variant).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Legal score for `winner`, picked by `variant` (see [`SCORES`]).
fn demo_score(variant: usize, winner: Side) -> Score {
    let sets = SCORES[variant % SCORES.len()]
        .iter()
        .map(|&(won, lost, match_tiebreak)| {
            let (side_a, side_b) = if winner == Side::A { (won, lost) } else { (lost, won) };
            SetScore { a: side_a, b: side_b, match_tiebreak }
        })
        .collect();
    Score { sets }
}

/// The first player of `side`.
fn lead(row: &MatchRow, side: Side) -> anyhow::Result<Uuid> {
    row.players(side).first().copied().context("a match side has no players")
}

/// Schedules `row` (a proposal by side A that side B accepts, as the API would), then, unless
/// it is only [`Fate::Scheduled`], has the winner report a score and the loser confirm or
/// dispute it. The favourite usually wins; every fourth match (by `variant`, which also
/// varies times, places and scores) is an upset.
async fn play(
    tx: &mut TenantTx,
    (now, window): (DateTime<Utc>, Duration),
    row: &MatchRow,
    (fate, favourite): (Fate, Side),
    variant: usize,
) -> anyhow::Result<()> {
    let hour = 17 + u32::try_from(variant % 3)?;
    let at = match fate {
        Fate::Confirmed => slot(now, 9 * i64::from(row.round.unwrap_or(1)) - 30, hour),
        Fate::Reported => slot(now, -1, 18),
        Fate::Disputed => slot(now, -4, 18),
        Fate::Scheduled => slot(now, 2 + i64::try_from(variant % 4)?, hour),
    };
    let location = LOCATIONS[variant % LOCATIONS.len()];
    let proposal =
        matches::insert_proposal(tx, row.id, lead(row, Side::A)?, at, Some(location)).await?;
    let accept = Event::AcceptProposal { proposed_by: Side::A };
    let status = row.transition(Actor::Player(Side::B), accept)?;
    // The accept endpoint only takes future times, so the played matches mirror what it writes.
    let _ = sqlx::query(
        "WITH accepted AS (
            UPDATE match_proposals SET status = 'accepted', updated_at = now()
            WHERE community_id = $1 AND id = $6)
         UPDATE matches SET status = $3, scheduled_at = $4, location = $5, updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(row.id)
    .bind(DbMatchStatus::from(status))
    .bind(at)
    .bind(location)
    .bind(proposal)
    .execute(&mut **tx)
    .await?;
    if matches!(fate, Fate::Scheduled) {
        return Ok(());
    }

    let side = if variant % 4 == 3 { favourite.other() } else { favourite };
    let score = demo_score(variant, side);
    let winner = results::check_score(&row.match_format, &score)?.winner;
    let scheduled = matches::load(tx, row.id, true).await?;
    let _ = scheduled.transition(Actor::Player(winner), Event::Report)?;
    let reported_at = at + Duration::hours(2);
    let reporter = lead(row, winner)?;
    results::record_report(tx, &scheduled, reporter, &score, winner, reported_at, window).await?;
    let reported = matches::load(tx, row.id, true).await?;
    let loser = Actor::Player(winner.other());
    match fate {
        Fate::Confirmed => {
            let status = reported.transition(loser, Event::Confirm)?;
            matches::set_status(tx, row.id, status).await?;
            let confirmed = matches::load(tx, row.id, false).await?;
            rankings::on_match_result(tx, &confirmed, reported_at + Duration::hours(20)).await?;
        }
        Fate::Disputed => {
            let status = reported.transition(loser, Event::Dispute)?;
            let _ = sqlx::query(
                "UPDATE matches SET status = $3, disputed_by = $4, dispute_note = $5,
                    updated_at = now()
                 WHERE community_id = $1 AND id = $2",
            )
            .bind(tx.community_id())
            .bind(row.id)
            .bind(DbMatchStatus::from(status))
            .bind(lead(row, winner.other())?)
            .bind("We played a deciding tiebreak; the score is wrong.")
            .execute(&mut **tx)
            .await?;
        }
        Fate::Reported | Fate::Scheduled => {}
    }
    Ok(())
}

/// Opens one singles and one doubles (with a partner) match request, unless their creators
/// already have a request at that location. `who` = singles creator, doubles creator and
/// partner.
async fn open_match_requests(
    state: &AppState,
    tenant: &Tenant,
    [single, host, partner]: [Uuid; 3],
) -> anyhow::Result<()> {
    let now = state.clock.now();
    // Creator, partner, discipline, UTR band in tenths, window, location.
    let calls = [
        (
            single,
            None,
            Discipline::Singles,
            Some((40, 65)),
            (slot(now, 2, 17), slot(now, 2, 20)),
            LOCATIONS[0],
        ),
        (
            host,
            Some(partner),
            Discipline::Doubles,
            None,
            (slot(now, 5, 9), slot(now, 5, 12)),
            LOCATIONS[1],
        ),
    ];
    let mut tx = tenant.begin(&state.db).await?;
    for (creator, partner, discipline, band, window, location) in calls {
        let slots = 2 * discipline.players_per_side() - 1 - usize::from(partner.is_some());
        let tenths = |pick: fn((i64, i64)) -> i64| band.map(|pair| Decimal::new(pick(pair), 1));
        let _ = sqlx::query(
            "INSERT INTO match_requests (id, community_id, created_by, discipline, partner_id,
                slots_open, utr_min, utr_max, time_window_start, time_window_end, location)
             SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11
             WHERE NOT EXISTS (SELECT 1 FROM match_requests
                WHERE community_id = $2 AND created_by = $3 AND location = $11)",
        )
        .bind(Uuid::now_v7())
        .bind(tenant.id())
        .bind(creator)
        .bind(DbDiscipline::from(discipline))
        .bind(partner)
        .bind(i32::try_from(slots)?)
        .bind(tenths(|(low, _)| low))
        .bind(tenths(|(_, high)| high))
        .bind(window.0)
        .bind(window.1)
        .bind(location)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// What [`SeedSummary::counts`] counts: a label and the `FROM` clause to count.
const COUNTS: [(&str, &str); 5] = [
    ("players", "players WHERE community_id = $1"),
    ("leagues", "leagues WHERE community_id = $1"),
    ("league matches", "matches WHERE community_id = $1"),
    ("open match requests", "match_requests WHERE community_id = $1 AND status = 'open'"),
    ("ranked players", "rankings WHERE community_id = $1"),
];

async fn summarize(state: &AppState, tenant: &Tenant) -> anyhow::Result<SeedSummary> {
    let mut tx = tenant.begin(&state.db).await?;
    let mut counts = Vec::new();
    for (what, from) in COUNTS {
        let sql = format!("SELECT count(*) FROM {from}");
        counts.push((what, sqlx::query_scalar(&sql).bind(tenant.id()).fetch_one(&mut *tx).await?));
    }
    tx.commit().await?;
    Ok(SeedSummary {
        community_id: tenant.id(),
        slug: tenant.slug.clone(),
        owner_email: email_of(OWNER.0),
        counts,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use racquetcollective_domain::{MatchFormat, validate_score};

    use super::*;
    use crate::communities::Branding;

    #[test]
    fn production_is_refused_in_any_spelling() {
        for env in ["production", "Production", " PRODUCTION ", "prod"] {
            let err = ensure_not_production(Some(env)).unwrap_err().to_string();
            assert!(err.contains("refusing to seed"), "{env}: {err}");
        }
    }

    #[test]
    fn other_environments_are_allowed() {
        for env in [None, Some(""), Some("development"), Some("staging"), Some("test")] {
            ensure_not_production(env).unwrap();
        }
    }

    #[test]
    fn every_demo_score_is_legal_for_either_winner() {
        let format = MatchFormat::default();
        for variant in 0..SCORES.len() {
            for winner in [Side::A, Side::B] {
                let summary = validate_score(&format, &demo_score(variant, winner)).unwrap();
                assert_eq!(summary.winner, winner, "variant {variant}");
            }
        }
    }

    #[test]
    fn branding_has_every_token() {
        let branding: Branding = serde_json::from_value(branding()).unwrap();
        assert_eq!(branding.display_name.as_deref(), Some(CLUB_NAME));
        assert!(branding.logo_url.is_some() && branding.typography.is_some());
        assert!(branding.colors.len() >= 3 && !branding.feature_flags.is_empty());
    }

    #[test]
    fn the_player_table_is_a_distinct_spread_of_utr() {
        let all: Vec<_> = std::iter::once(OWNER).chain(PLAYERS).collect();
        let emails: HashSet<_> = all.iter().map(|row| email_of(row.0)).collect();
        assert_eq!(emails.len(), all.len(), "emails are unique");
        assert!(PLAYERS.windows(2).all(|pair| pair[0].1 <= pair[1].1));
        assert_eq!((PLAYERS[0].1, PLAYERS[23].1), (30, 80));
        assert!(all.iter().any(|row| row.2 == Gender::Other));
        assert!(all.iter().any(|row| row.2 == Gender::Undisclosed));
    }
}
