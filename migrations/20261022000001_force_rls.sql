-- Row-level security for the table owner too (decision 110).
--
-- Where the login role cannot create `courtpit_app` (RlsMode::Owner), tenant transactions run as
-- the owner, which RLS skips unless forced. FORCE makes the policies apply to the owner as well;
-- a superuser or a BYPASSRLS role (Neon's owner, the test harness) still skips them.
--
-- Work that must see every community (job sweeps, account deletion, backups) opens a
-- transaction with `app.rls_bypass = 'on'` (`GlobalTx`), which the `rls_bypass` policy admits.
-- Permissive policies combine with OR, so tenant transactions keep `tenant_isolation`.

CREATE FUNCTION app_rls_bypass() RETURNS boolean
    LANGUAGE sql STABLE
    AS $$ SELECT coalesce(current_setting('app.rls_bypass', true), '') = 'on' $$;

DO $$
DECLARE
    tbl text;
BEGIN
    FOREACH tbl IN ARRAY ARRAY[
        'players', 'matches', 'match_proposals', 'match_requests', 'match_request_joins',
        'leagues', 'league_divisions', 'league_entries', 'ranking_events', 'rankings',
        'league_results', 'device_tokens', 'notification_prefs'
    ] LOOP
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', tbl);
        EXECUTE format(
            'CREATE POLICY rls_bypass ON %I USING (app_rls_bypass()) WITH CHECK (app_rls_bypass())',
            tbl
        );
    END LOOP;
END
$$;
