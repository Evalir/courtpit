-- Final standings of a finished league season, with promotion/relegation for the next one.

CREATE TABLE league_results (
    community_id uuid NOT NULL,
    league_id    uuid NOT NULL,
    entry_id     uuid NOT NULL,
    division_id  uuid NOT NULL,
    -- Copied from the entry so the next season can recognise returning players/pairs.
    player_ids   uuid[] NOT NULL,
    tier         int NOT NULL,
    position     int NOT NULL CHECK (position >= 1),
    -- League-match points earned in the box.
    points       int NOT NULL,
    -- Season ranking points awarded to each player of the entry.
    season_points int NOT NULL,
    -- -1 promoted (towards tier 1), +1 relegated, 0 stays.
    movement     smallint NOT NULL CHECK (movement BETWEEN -1 AND 1),
    created_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (league_id, entry_id),
    FOREIGN KEY (community_id, league_id) REFERENCES leagues (community_id, id) ON DELETE CASCADE,
    FOREIGN KEY (community_id, entry_id) REFERENCES league_entries (community_id, id),
    FOREIGN KEY (community_id, division_id) REFERENCES league_divisions (community_id, id)
);

ALTER TABLE league_results ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON league_results
    USING (community_id = app_current_community())
    WITH CHECK (community_id = app_current_community());

GRANT SELECT, INSERT, UPDATE, DELETE ON league_results TO courtpit_app;
