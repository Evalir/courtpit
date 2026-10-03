-- Matches: friendly, league and (later) tournament.
-- One table and one state machine for every kind of match; sides are arrays of player ids so
-- singles and doubles share the shape. Element-level FKs aren't possible on arrays; the server
-- validates membership when it writes them.

CREATE TYPE discipline AS ENUM ('singles', 'doubles', 'mixed');
CREATE TYPE match_status AS ENUM (
    'proposed', 'scheduled', 'reported', 'confirmed', 'disputed', 'resolved', 'walkover',
    'cancelled'
);
CREATE TYPE match_side AS ENUM ('a', 'b');

CREATE TABLE matches (
    id                  uuid PRIMARY KEY,
    community_id        uuid NOT NULL REFERENCES communities (id) ON DELETE CASCADE,
    discipline          discipline NOT NULL,
    -- Competitive context; FKs are added by the leagues/tournaments migrations.
    league_id           uuid,
    division_id         uuid,
    tournament_id       uuid,
    round               int CHECK (round >= 1),
    side_a_players      uuid[] NOT NULL,
    side_b_players      uuid[] NOT NULL,
    status              match_status NOT NULL DEFAULT 'proposed',
    scheduled_at        timestamptz,
    location            text,
    -- Snapshot of the format in force when the match was created (community default or
    -- league override), so later rule edits don't invalidate reported scores.
    match_format        jsonb NOT NULL,
    score               jsonb,
    winner_side         match_side,
    reported_by         uuid,
    reported_at         timestamptz,
    confirm_deadline_at timestamptz,
    resolved_by         uuid,
    resolution_note     text,
    created_by          uuid,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    UNIQUE (community_id, id),
    CHECK (cardinality(side_a_players) = CASE discipline WHEN 'singles' THEN 1 ELSE 2 END),
    CHECK (cardinality(side_b_players) = cardinality(side_a_players)),
    CHECK (NOT (side_a_players && side_b_players)),
    CHECK (tournament_id IS NULL OR league_id IS NULL),
    CHECK ((score IS NULL) = (winner_side IS NULL) OR status = 'walkover')
);
-- "My matches": (side_a_players || side_b_players) @> ARRAY[player_id].
CREATE INDEX matches_players_idx ON matches USING gin ((side_a_players || side_b_players));
CREATE INDEX matches_community_status_idx ON matches (community_id, status);
CREATE INDEX matches_league_idx ON matches (league_id, division_id) WHERE league_id IS NOT NULL;

ALTER TABLE matches ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON matches
    USING (community_id = app_current_community())
    WITH CHECK (community_id = app_current_community());

GRANT SELECT, INSERT, UPDATE, DELETE ON matches TO courtpit_app;
