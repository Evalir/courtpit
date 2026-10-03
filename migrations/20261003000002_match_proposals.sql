-- Time-and-place proposals for a match. Either side proposes; the other accepts or
-- counter-proposes. At most one proposal is open at a time.

-- `superseded`: replaced by a newer (counter-)proposal, or the match was cancelled, before
-- anyone answered it.
CREATE TYPE proposal_status AS ENUM ('open', 'accepted', 'declined', 'superseded');

CREATE TABLE match_proposals (
    id            uuid PRIMARY KEY,
    community_id  uuid NOT NULL,
    match_id      uuid NOT NULL,
    proposed_by   uuid NOT NULL,
    proposed_time timestamptz NOT NULL,
    location      text,
    status        proposal_status NOT NULL DEFAULT 'open',
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (community_id, match_id) REFERENCES matches (community_id, id) ON DELETE CASCADE,
    FOREIGN KEY (community_id, proposed_by) REFERENCES players (community_id, id)
);
CREATE INDEX match_proposals_match_idx ON match_proposals (match_id, created_at);
CREATE UNIQUE INDEX match_proposals_one_open ON match_proposals (match_id) WHERE status = 'open';

ALTER TABLE match_proposals ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON match_proposals
    USING (community_id = app_current_community())
    WITH CHECK (community_id = app_current_community());

GRANT SELECT, INSERT, UPDATE, DELETE ON match_proposals TO courtpit_app;
