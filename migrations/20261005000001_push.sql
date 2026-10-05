-- Push notifications (spec §14, build step 6).
--
-- Each community ships its own app (spec §16), so an Expo push token belongs to one community's
-- app: tokens are kept per membership (player), not per global user as the spec's sketch has it
-- (decision 96). A device that signs in as someone else moves its token to them.
CREATE TYPE device_platform AS ENUM ('ios', 'android');

CREATE TABLE device_tokens (
    id              uuid PRIMARY KEY,
    community_id    uuid NOT NULL REFERENCES communities (id) ON DELETE CASCADE,
    player_id       uuid NOT NULL,
    expo_push_token text NOT NULL,
    platform        device_platform NOT NULL,
    last_seen_at    timestamptz NOT NULL DEFAULT now(),
    created_at      timestamptz NOT NULL DEFAULT now(),
    UNIQUE (community_id, expo_push_token),
    FOREIGN KEY (community_id, player_id) REFERENCES players (community_id, id) ON DELETE CASCADE
);
CREATE INDEX device_tokens_player_idx ON device_tokens (community_id, player_id);

-- Which optional notifications a member wants. No row means everything on.
CREATE TABLE notification_prefs (
    player_id      uuid PRIMARY KEY,
    community_id   uuid NOT NULL,
    match_updates  boolean NOT NULL DEFAULT true,
    league_updates boolean NOT NULL DEFAULT true,
    reminders      boolean NOT NULL DEFAULT true,
    updated_at     timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (community_id, player_id) REFERENCES players (community_id, id) ON DELETE CASCADE
);

ALTER TABLE device_tokens ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON device_tokens
    USING (community_id = app_current_community())
    WITH CHECK (community_id = app_current_community());
ALTER TABLE notification_prefs ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON notification_prefs
    USING (community_id = app_current_community())
    WITH CHECK (community_id = app_current_community());

GRANT SELECT, INSERT, UPDATE, DELETE ON device_tokens, notification_prefs TO courtpit_app;
