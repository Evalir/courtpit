# Racquet Collective — notes for agents and contributors

Spec: `docs/architecture.md` (authoritative). Where it is silent, decide, and record the
decision in `docs/decisions.md`. Read the relevant spec section before changing behaviour.

## Layout
- `crates/domain` — pure logic. **No IO, no tokio, no sqlx.** Score validation, match state
  machine, placement/pairings, scoring rules. Exhaustive unit tests live next to the code.
- `crates/server` — axum 0.8 + sqlx 0.8 (Postgres) + utoipa. Handlers are thin: parse, call
  `domain`, persist. Single binary `racquetcollective-server` with subcommands `serve`, `tick`,
  `migrate`, `create-community`, `openapi`, `seed` (demo data; refuses
  `RACQUETCOLLECTIVE_ENV=production`).
- `apps/mobile` — the Expo app (iOS, Android, web); design and conventions in
  `docs/frontend.md`. `packages/api-client` — the generated TypeScript client. Both are npm
  workspaces of the root `package.json` (one root `package-lock.json`; run `npm ci` at the root).
- `migrations/` — sqlx migrations embedded with `sqlx::migrate!()`. **One new migration file per
  PR that touches schema. Never edit a migration that has shipped** (anything below the stack tip).

## Binding conventions
- **sqlx: runtime-checked queries only** — `sqlx::query_as::<_, T>(..)` / `sqlx::query(..)` with
  `#[derive(FromRow)]`. Do **not** use `query!`/`query_as!` macros (their `.sqlx` offline data
  churns on every stacked PR). Correctness comes from integration tests against real Postgres.
- **IDs are UUID v7**, generated in Rust (`Uuid::now_v7()`); Postgres 16 has no `uuidv7()`.
- **Enums** are Postgres `CREATE TYPE … AS ENUM` types mapped with `#[derive(sqlx::Type)]`
  (`rename_all = "snake_case"`/`"lowercase"`), not text + CHECK. Adding a value is
  `ALTER TYPE … ADD VALUE` in a new migration. Exception: `jobs.kind` is free text (an open set).
- **Time**: `chrono::DateTime<Utc>` / `timestamptz` everywhere. Business logic reads the
  current time from `state.clock.now()` (an injectable `Clock`) and binds it into SQL, never
  Postgres `now()`, so tests can move time with `app.clock.advance(..)`. Audit columns
  (`created_at`, `updated_at`) and auth expiry may keep using `now()`.
- **Errors**: return `ApiError`; every error renders as
  `{ "error": { "code": "<stable_code>", "message": "<human text>" } }` with the matching HTTP
  status. Clients switch on `code`. Never leak internal error text (`Internal` logs, then says
  "internal error").
- **No `unwrap()`/`expect()` in non-test server code** (clippy `unwrap_used` is denied via
  `-D warnings`). `unsafe_code` is denied workspace-wide.
- **Tenancy**: every tenant-scoped table has `community_id` and RLS policies keyed on
  `nullif(current_setting('app.community_id', true), '')::uuid`. Handlers take `Tenant`; all
  tenant-scoped queries run inside `TenantTx`, which does `SET LOCAL app.community_id` and
  `SET LOCAL ROLE courtpit_app` (a NOBYPASSRLS, non-owner role) so RLS actually applies.
  RLS is the backstop, not the filter: every query on a scoped table still says
  `WHERE community_id = $n` explicitly (bind `tenant.id()` / `tx.community_id()`).
  Global tables (`users`, `sessions`, `email_codes`, `auth_identities`, `communities`, `jobs`)
  are queried on the pool directly.
- **OpenAPI**: every handler has `#[utoipa::path]` and is registered with `routes!` in
  `app::api_router`, so `/api/v1/openapi.json` stays complete. Authenticated handlers declare
  `security(("bearer" = []))`; query structs derive `IntoParams` with
  `#[into_params(parameter_in = Query)]`; tags are declared on `openapi::ApiDoc`; unit tests
  in `openapi.rs` check the generated document. Any PR that changes the API surface re-runs
  `npm run gen` in `packages/api-client` and commits `openapi.json` + `src/schema.d.ts`
  (CI's `api-client` step fails on drift).
- **Lists** use cursor pagination (`?cursor=&limit=`, response `{ items, next_cursor }`).
- **Money**: no payment logic in steps 1–3. The spec's payment columns exist as an unused
  seam (`communities.currency/stripe_*/platform_fee_*`, `leagues.entry_fee_minor`).

## Frontend conventions (`apps/mobile`)
- Expo SDK 57 APIs move between releases: check the versioned docs
  (`https://docs.expo.dev/versions/v57.0.0/`) rather than memory, and add native packages with
  `npx expo install` (then move dev tools to `devDependencies`; it puts everything in
  `dependencies`).
- Routes only in `src/app`; everything else in `src/{api,session,tenant,theme,ui,features}`.
- Data: `useApi().$api.useQuery/useMutation` (typed from the spec); cursor lists via
  `useCursorList`; after a write call `refreshAfterWrite`. Never hand-write API types.
- Styling: `createStyles(theme => …)` and the `src/ui` primitives; colors only from the theme
  palette (they are the community's branding), never literals in screens.
- Pure logic (grouping, formatting, color math) lives in plain functions with unit tests next to
  them; screens stay thin.

## Local environment
- Postgres 16: `DATABASE_URL=postgres://racquetcollective:racquetcollective@127.0.0.1/racquetcollective` (see `.env.example`;
  the role must be allowed to create databases for the test harness). `service postgresql start`
  if it is not running.
- Share one target dir across worktrees to reuse compiled deps:
  `export CARGO_TARGET_DIR=/home/claude/racquetcollective-target`.
- No `sqlx-cli` needed: `cargo run -p racquetcollective-server -- migrate` applies migrations.

## Tests
- `domain`: plain unit tests (`cargo test -p racquetcollective-domain`).
- `server`: one integration test binary `crates/server/tests/it/` (a module per area).
  **Per-test database cloned from a migrated template**: the harness migrates a template DB
  named `racquetcollective_tpl_<hash of migrations>` once (under an advisory lock), then every test runs
  `CREATE DATABASE racquetcollective_test_<uuid> TEMPLATE ...` — fast and fully isolated. Leftover test
  databases from earlier runs are dropped at harness start. Needs `DATABASE_URL` pointing at a
  role allowed to create databases (default `postgres://racquetcollective:racquetcollective@127.0.0.1/racquetcollective`).
- CI on `main` (not on pull requests) also runs the suite through pgbouncer (transaction pooling, `ci/pgbouncer/`) with
  `DATABASE_DIRECT_URL` for the harness's admin work and `RACQUETCOLLECTIVE_DB_POOLED=true`; see
  `docs/deploy.md`. Keep server code free of session state (session `SET`, advisory locks,
  `LISTEN`, temp tables): `SET LOCAL` inside a transaction is fine.
- Use the helpers in `tests/it/common.rs` (`TestApp::spawn`, `app.req(..)`, `app.login(..)`)
  rather than hand-rolling requests.

## Quality gate (every PR, must be green)
CI checks only pull requests that are ready for review and target `main` (plus every push to
`main`): drafts and stacked PRs above the bottom get no CI, so run the gate locally before every
push. To check another branch in CI, run the CI workflow on it by hand (Actions → CI → Run
workflow); minutes are scarce, so only when the local gate can't (decision 106).
```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
```
PRs touching `apps/mobile` (from `apps/mobile`, after `npm ci` at the root):
```sh
npm run format:check && npm run lint && npm run typecheck && npm test && npm run export:web
```

## Lints
- The strict set lives in `[workspace.lints]` (root `Cargo.toml`, borrowed from init4's
  spellbook/jay/signet-sdk) plus thresholds in `clippy.toml`. Highlights: `missing_docs` on
  every public item, `unreachable_pub` (use `pub(crate)`, including in `tests/it`),
  `unused_results` (bind ignored values with `let _ =`), `min_ident_chars` (no `|e|`/`|r|`),
  `missing_const_for_fn`, `mod_module_files` (`foo.rs` + `foo/`, never `foo/mod.rs`),
  `too_many_lines` (100), `rustdoc::all`.
- Each library crate has `#![cfg_attr(not(test), warn(unused_crate_dependencies))]`: add a
  dependency in the PR that first uses it.
- Silence a lint at the narrowest scope with `#[expect(lint, reason = "...")]`; bare
  `#[allow]` is itself a lint error.

## Stacked PRs
- Linear stack `cp/01-…`, `cp/02-…`; each branch starts at the previous branch's tip; PR N
  targets branch N-1, PR 01 targets `main`. Keep PRs focused (~200–600 changed lines, excluding
  `Cargo.lock`).
- Fixing a lower PR: commit on its branch, then `git rebase --onto <new-base> <old-base> <branch>`
  for every branch above it.
- Every commit message ends with the two trailer lines:
  ```
  Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01DpAja1DpzgBswgjV7HmpwM
  ```
