-- The ranking ledger and its materialised 52-week view.
-- Every scoring event appends a ranking_events row; rankings is rebuilt from the ledger by
-- the refresh job, so a rule change is a ledger replay rather than a data fix.

CREATE TYPE ranking_source AS ENUM ('league_match', 'league_season', 'tournament');

CREATE TABLE ranking_events (
    id           uuid PRIMARY KEY,
    community_id uuid NOT NULL REFERENCES communities (id) ON DELETE CASCADE,
    player_id    uuid NOT NULL,
    -- The discipline played; mixed_pooling is applied when rankings are computed.
    discipline   discipline NOT NULL,
    source       ranking_source NOT NULL,
    -- The match (league_match), league (league_season) or tournament that earned the points.
    source_id    uuid NOT NULL,
    points       int NOT NULL CHECK (points >= 0),
    occurred_at  timestamptz NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (community_id, player_id) REFERENCES players (community_id, id),
    -- Writing events is idempotent: one per player per source row.
    UNIQUE (source, source_id, player_id)
);
CREATE INDEX ranking_events_window_idx ON ranking_events (community_id, occurred_at);
CREATE INDEX ranking_events_player_idx ON ranking_events (community_id, player_id, occurred_at);

CREATE TABLE rankings (
    community_id uuid NOT NULL REFERENCES communities (id) ON DELETE CASCADE,
    player_id    uuid NOT NULL,
    discipline   discipline NOT NULL,
    points_52w   int NOT NULL,
    rank         int NOT NULL,
    refreshed_at timestamptz NOT NULL,
    PRIMARY KEY (community_id, discipline, player_id),
    FOREIGN KEY (community_id, player_id) REFERENCES players (community_id, id)
);
CREATE INDEX rankings_order_idx ON rankings (community_id, discipline, rank);

ALTER TABLE ranking_events ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON ranking_events
    USING (community_id = app_current_community())
    WITH CHECK (community_id = app_current_community());
ALTER TABLE rankings ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON rankings
    USING (community_id = app_current_community())
    WITH CHECK (community_id = app_current_community());

GRANT SELECT, INSERT, UPDATE, DELETE ON ranking_events, rankings TO courtpit_app;
