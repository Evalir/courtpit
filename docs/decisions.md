# Decisions beyond the architecture doc

Choices made where `docs/architecture.md` is silent. Newest at the bottom. Each entry: what,
why, and the PR that introduced it.

| # | Decision | Why | PR |
|---|---|---|---|
| 1 | Runtime-checked sqlx queries (`query_as::<_, T>`) instead of the compile-time macros the doc mentions. | `.sqlx` offline data conflicts on every rebase of a stacked PR; integration tests against real Postgres give the same safety. | 01 |
| 2 | Errors render as `{ "error": { "code", "message" } }` (`application/json`) with the matching HTTP status; `code` is a stable snake_case identifier. | One shape for every failure; clients switch on `code`. | 01 |
| 3 | IDs are UUID v7 generated in Rust. | Time-ordered (good index locality, usable as pagination cursor); PG16 has no native v7. | 01 |
| 4 | Single integration test binary (`tests/it`) with modules. | One link step instead of one per file. | 01 |
| 5 | Tenant-scoped queries run as `courtpit_app` (NOLOGIN, NOBYPASSRLS, not a table owner) via `SET LOCAL ROLE` inside `TenantTx`; the pool's login role owns the schema and is granted `courtpit_app`. Policies use `app_current_community()` = `nullif(current_setting('app.community_id', true), '')::uuid`. | One pool; RLS can't be bypassed by the owner role on the scoped path; an unset or reset GUC yields no rows instead of a cast error. Production: the migrating/login role needs `CREATEROLE` once (or create `courtpit_app` out of band) and must be granted `courtpit_app`. | 02 |
| 6 | Integration tests clone a per-test database from a template migrated once per process (`courtpit_tpl_<hash of migrations>`, built under an advisory lock and renamed into place); stale test DBs are dropped at harness start. | Isolation of a fresh DB at the cost of a file copy rather than a full migration per test. | 02 |
| 7 | `users.email` is unique case-insensitively (`lower(email)` index). `player_status` adds `deleted` for self-service account deletion (anonymise, keep match history consistent). Tenant parents also carry `UNIQUE (community_id, id)` so children can use composite FKs that pin them to the same community. | Spec silent. | 02 |
| 8 | Tenant resolution order: `X-Courtpit-Community` header, then Host `{slug}.{COURTPIT_BASE_DOMAIN}` (single label only), then Host as a registered `custom_domain`. Missing → 400, unknown → 404. Resolved communities are cached 60 s (moka); misses are not cached. | Header for native builds, host for web; short TTL lets branding edits propagate without invalidation plumbing. | 03 |
