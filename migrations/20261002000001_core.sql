-- Core identity and tenancy schema: users, auth, sessions, communities, players.
-- Also creates the `courtpit_app` runtime role that tenant-scoped queries run as, so that
-- row-level security applies (superusers and table owners bypass RLS).

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'courtpit_app') THEN
        CREATE ROLE courtpit_app NOLOGIN NOSUPERUSER NOBYPASSRLS NOINHERIT;
    END IF;
END
$$;

-- The connecting role must be able to `SET ROLE courtpit_app`.
GRANT courtpit_app TO CURRENT_USER;
GRANT USAGE ON SCHEMA public TO courtpit_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO courtpit_app;

-- The community the current transaction is scoped to (set by TenantTx with SET LOCAL).
-- NULL when unset, so policies match nothing rather than erroring.
CREATE FUNCTION app_current_community() RETURNS uuid
    LANGUAGE sql STABLE
    AS $$ SELECT nullif(current_setting('app.community_id', true), '')::uuid $$;

CREATE TYPE auth_provider AS ENUM ('apple', 'google');
CREATE TYPE email_code_purpose AS ENUM ('verify', 'login');
CREATE TYPE join_policy AS ENUM ('open');
CREATE TYPE player_gender AS ENUM ('female', 'male', 'other', 'undisclosed');
CREATE TYPE play_pref AS ENUM ('singles', 'doubles', 'any');
CREATE TYPE player_role AS ENUM ('player', 'admin', 'owner');
CREATE TYPE player_status AS ENUM ('active', 'banned', 'deleted');

-- Global login identity.
CREATE TABLE users (
    id                uuid PRIMARY KEY,
    email             text NOT NULL,
    email_verified_at timestamptz,
    password_hash     text,
    deleted_at        timestamptz,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX users_email_key ON users (lower(email));

CREATE TABLE auth_identities (
    id         uuid PRIMARY KEY,
    user_id    uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    provider   auth_provider NOT NULL,
    subject    text NOT NULL,
    email      text,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (provider, subject)
);
CREATE INDEX auth_identities_user_id_idx ON auth_identities (user_id);

-- Opaque session tokens; only the SHA-256 of the token is stored.
CREATE TABLE sessions (
    token_hash   bytea PRIMARY KEY,
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    device_label text,
    expires_at   timestamptz NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX sessions_user_id_idx ON sessions (user_id);

-- One-time email codes (hashed), 10-minute expiry, limited attempts.
CREATE TABLE email_codes (
    id          uuid PRIMARY KEY,
    user_id     uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    code_hash   bytea NOT NULL,
    purpose     email_code_purpose NOT NULL,
    attempts    int NOT NULL DEFAULT 0,
    expires_at  timestamptz NOT NULL,
    consumed_at timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX email_codes_user_id_created_idx ON email_codes (user_id, created_at DESC);

-- Tenants.
CREATE TABLE communities (
    id                   uuid PRIMARY KEY,
    slug                 text NOT NULL UNIQUE
                         CHECK (slug ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    name                 text NOT NULL,
    custom_domain        text UNIQUE,
    branding             jsonb NOT NULL DEFAULT '{}',
    settings             jsonb NOT NULL DEFAULT '{}',
    join_policy          join_policy NOT NULL DEFAULT 'open',
    scoring_config       jsonb NOT NULL DEFAULT '{
        "league_match": {"win_straight": 3, "win_deciding": 2, "loss_deciding": 1,
                         "loss_straight": 0, "walkover_win": 2, "walkover_loss": 0},
        "league_season": {"position_points": [100, 70, 50, 35, 25, 15, 10, 5],
                          "tier_multiplier": {"1": 1.0, "2": 0.7, "3": 0.5, "4": 0.35}},
        "tournament": {"round_points_pct": {"winner": 100, "final": 60, "semi": 36,
                                            "quarter": 18, "r16": 9, "r32": 4},
                       "base_by_draw_size": {"8": 100, "16": 150, "32": 250}},
        "mixed_pooling": "separate"
    }',
    default_match_format jsonb NOT NULL DEFAULT
        '{"sets_to_win": 2, "games_per_set": 6, "tiebreak_at": 6, "final_set": "match_tiebreak_10"}',
    -- Payments seam (step 4, Stripe Connect). Present per the spec, unused until then.
    currency                   text NOT NULL DEFAULT 'EUR' CHECK (currency ~ '^[A-Z]{3}$'),
    stripe_account_id          text,
    stripe_onboarding_complete boolean NOT NULL DEFAULT false,
    platform_fee_bps           int NOT NULL DEFAULT 0 CHECK (platform_fee_bps BETWEEN 0 AND 10000),
    platform_fee_fixed_minor   int NOT NULL DEFAULT 0 CHECK (platform_fee_fixed_minor >= 0),
    created_at           timestamptz NOT NULL DEFAULT now(),
    updated_at           timestamptz NOT NULL DEFAULT now()
);

-- A user's membership in one community, carrying the tennis profile. Tenant-scoped.
CREATE TABLE players (
    id                  uuid PRIMARY KEY,
    community_id        uuid NOT NULL REFERENCES communities (id) ON DELETE CASCADE,
    user_id             uuid NOT NULL REFERENCES users (id),
    display_name        text NOT NULL,
    utr                 numeric(4, 2) CHECK (utr >= 1 AND utr <= 16.5),
    gender              player_gender NOT NULL DEFAULT 'undisclosed',
    phone               text,
    phone_visible       boolean NOT NULL DEFAULT false,
    socials             jsonb NOT NULL DEFAULT '{}',
    socials_visible     boolean NOT NULL DEFAULT false,
    racket              text,
    strings             text,
    tension_kg          numeric(4, 1),
    play_pref           play_pref NOT NULL DEFAULT 'any',
    preferred_locations jsonb NOT NULL DEFAULT '[]',
    role                player_role NOT NULL DEFAULT 'player',
    status              player_status NOT NULL DEFAULT 'active',
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    UNIQUE (community_id, user_id),
    UNIQUE (community_id, id)
);
CREATE INDEX players_user_id_idx ON players (user_id);

ALTER TABLE players ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON players
    USING (community_id = app_current_community())
    WITH CHECK (community_id = app_current_community());

-- Default privileges cover tables created later by this role; be explicit for these too.
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO courtpit_app;
