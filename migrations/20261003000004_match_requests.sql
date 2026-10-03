-- Open calls for a friendly match ("doubles, Saturday morning, UTR 4–6, two slots open").
-- Others join; when slots_open reaches zero the request is filled and a match is created.

CREATE TYPE match_request_status AS ENUM ('open', 'filled', 'cancelled');

CREATE TABLE match_requests (
    id                uuid PRIMARY KEY,
    community_id      uuid NOT NULL REFERENCES communities (id) ON DELETE CASCADE,
    created_by        uuid NOT NULL,
    discipline        discipline NOT NULL,
    -- Bring-your-own partner for doubles; otherwise the first joiner partners the creator.
    partner_id        uuid,
    slots_open        int NOT NULL CHECK (slots_open >= 0),
    utr_min           numeric(4, 2),
    utr_max           numeric(4, 2),
    time_window_start timestamptz NOT NULL,
    time_window_end   timestamptz NOT NULL,
    location          text,
    status            match_request_status NOT NULL DEFAULT 'open',
    -- The match created when the request filled.
    match_id          uuid,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    UNIQUE (community_id, id),
    FOREIGN KEY (community_id, created_by) REFERENCES players (community_id, id),
    FOREIGN KEY (community_id, partner_id) REFERENCES players (community_id, id),
    FOREIGN KEY (community_id, match_id) REFERENCES matches (community_id, id),
    CHECK (time_window_end > time_window_start),
    CHECK (utr_min IS NULL OR utr_max IS NULL OR utr_min <= utr_max),
    CHECK (partner_id IS NULL OR discipline <> 'singles'),
    CHECK ((status = 'filled') = (match_id IS NOT NULL))
);
CREATE INDEX match_requests_open_idx ON match_requests (community_id, time_window_end)
    WHERE status = 'open';

CREATE TABLE match_request_joins (
    community_id uuid NOT NULL,
    request_id   uuid NOT NULL,
    player_id    uuid NOT NULL,
    joined_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (request_id, player_id),
    FOREIGN KEY (community_id, request_id) REFERENCES match_requests (community_id, id)
        ON DELETE CASCADE,
    FOREIGN KEY (community_id, player_id) REFERENCES players (community_id, id)
);

ALTER TABLE match_requests ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON match_requests
    USING (community_id = app_current_community())
    WITH CHECK (community_id = app_current_community());
ALTER TABLE match_request_joins ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON match_request_joins
    USING (community_id = app_current_community())
    WITH CHECK (community_id = app_current_community());

GRANT SELECT, INSERT, UPDATE, DELETE ON match_requests, match_request_joins TO courtpit_app;
