-- Seasonal leagues, their divisions ("boxes") and entries. Entries, not players, are the
-- unit of participation: player_ids has one element for singles and two for doubles/mixed.

CREATE TYPE league_status AS ENUM ('draft', 'registration', 'active', 'finished', 'cancelled');
CREATE TYPE entry_status AS ENUM ('pending_partner', 'pending_payment', 'confirmed', 'withdrawn');

CREATE TABLE leagues (
    id                     uuid PRIMARY KEY,
    community_id           uuid NOT NULL REFERENCES communities (id) ON DELETE CASCADE,
    name                   text NOT NULL,
    discipline             discipline NOT NULL,
    registration_opens_at  timestamptz NOT NULL,
    registration_closes_at timestamptz NOT NULL,
    starts_at              timestamptz NOT NULL,
    ends_at                timestamptz NOT NULL,
    status                 league_status NOT NULL DEFAULT 'draft',
    -- Set by an admin when the league goes public; the lifecycle job only moves published
    -- leagues out of draft.
    published_at           timestamptz,
    -- NULL = the community's default format.
    match_format           jsonb,
    -- Payments seam (step 4): unused until then, so every entry confirms without payment.
    entry_fee_minor        int CHECK (entry_fee_minor >= 0),
    -- Partial scoring_config merged over the community's.
    scoring_overrides      jsonb,
    box_min_size           int NOT NULL DEFAULT 6,
    box_max_size           int NOT NULL DEFAULT 8,
    -- Last season, whose promotion/relegation seeds this one's placement.
    previous_league_id     uuid,
    created_by             uuid,
    created_at             timestamptz NOT NULL DEFAULT now(),
    updated_at             timestamptz NOT NULL DEFAULT now(),
    UNIQUE (community_id, id),
    FOREIGN KEY (community_id, previous_league_id) REFERENCES leagues (community_id, id),
    CHECK (registration_opens_at < registration_closes_at),
    CHECK (registration_closes_at <= starts_at),
    CHECK (starts_at < ends_at),
    CHECK (box_min_size >= 2 AND box_min_size <= box_max_size AND box_max_size <= 16)
);
CREATE INDEX leagues_community_status_idx ON leagues (community_id, status);

CREATE TABLE league_divisions (
    id           uuid PRIMARY KEY,
    community_id uuid NOT NULL,
    league_id    uuid NOT NULL,
    name         text NOT NULL,
    tier         int NOT NULL CHECK (tier >= 1),
    utr_min      numeric(4, 2),
    utr_max      numeric(4, 2),
    created_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (community_id, id),
    UNIQUE (league_id, tier),
    FOREIGN KEY (community_id, league_id) REFERENCES leagues (community_id, id) ON DELETE CASCADE
);

CREATE TABLE league_entries (
    id                  uuid PRIMARY KEY,
    community_id        uuid NOT NULL,
    league_id           uuid NOT NULL,
    division_id         uuid,
    player_ids          uuid[] NOT NULL,
    created_by          uuid NOT NULL,
    status              entry_status NOT NULL,
    looking_for_partner boolean NOT NULL DEFAULT false,
    -- Doubles/mixed: the partner named by the creator who hasn't accepted yet.
    invited_partner_id  uuid,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    UNIQUE (community_id, id),
    FOREIGN KEY (community_id, league_id) REFERENCES leagues (community_id, id) ON DELETE CASCADE,
    FOREIGN KEY (community_id, division_id) REFERENCES league_divisions (community_id, id),
    FOREIGN KEY (community_id, created_by) REFERENCES players (community_id, id),
    FOREIGN KEY (community_id, invited_partner_id) REFERENCES players (community_id, id),
    CHECK (cardinality(player_ids) BETWEEN 1 AND 2),
    CHECK (invited_partner_id IS NULL OR cardinality(player_ids) = 1)
);
CREATE INDEX league_entries_league_idx ON league_entries (league_id, status);
CREATE INDEX league_entries_players_idx ON league_entries USING gin (player_ids);

-- League matches point at their league and division.
ALTER TABLE matches
    ADD FOREIGN KEY (community_id, league_id) REFERENCES leagues (community_id, id),
    ADD FOREIGN KEY (community_id, division_id) REFERENCES league_divisions (community_id, id);

ALTER TABLE leagues ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON leagues
    USING (community_id = app_current_community())
    WITH CHECK (community_id = app_current_community());
ALTER TABLE league_divisions ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON league_divisions
    USING (community_id = app_current_community())
    WITH CHECK (community_id = app_current_community());
ALTER TABLE league_entries ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON league_entries
    USING (community_id = app_current_community())
    WITH CHECK (community_id = app_current_community());

GRANT SELECT, INSERT, UPDATE, DELETE ON leagues, league_divisions, league_entries TO courtpit_app;
