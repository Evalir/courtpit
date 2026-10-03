# Courtpit — notes for agents and contributors

Spec: `docs/architecture.md` (authoritative). Where it is silent, decide, and record the
decision in `docs/decisions.md`. Read the relevant spec section before changing behaviour.

## Layout
- `crates/domain` — pure logic. **No IO, no tokio, no sqlx.** Score validation, match state
  machine, placement/pairings, scoring rules. Exhaustive unit tests live next to the code.
- `crates/server` — axum 0.8 + sqlx 0.8 (Postgres) + utoipa. Handlers are thin: parse, call
  `domain`, persist. Single binary `courtpit-server` with subcommands `serve`, `migrate`,
  `create-community`.
- `migrations/` — sqlx migrations embedded with `sqlx::migrate!()`. **One new migration file per
  PR that touches schema. Never edit a migration that has shipped** (anything below the stack tip).

## Binding conventions
- **sqlx: runtime-checked queries only** — `sqlx::query_as::<_, T>(..)` / `sqlx::query(..)` with
  `#[derive(FromRow)]`. Do **not** use `query!`/`query_as!` macros (their `.sqlx` offline data
  churns on every stacked PR). Correctness comes from integration tests against real Postgres.
- **IDs are UUID v7**, generated in Rust (`Uuid::now_v7()`); Postgres 16 has no `uuidv7()`.
- **Time**: `chrono::DateTime<Utc>` / `timestamptz` everywhere.
- **Errors**: return `ApiError`; every error renders as
  `{ "error": { "code": "<stable_code>", "message": "<human text>" } }` with the matching HTTP
  status. Clients switch on `code`. Never leak internal error text (`Internal` logs, then says
  "internal error").
- **No `unwrap()`/`expect()` in non-test server code** (clippy `unwrap_used` is denied via
  `-D warnings`). `#![deny(unsafe_code)]` in every crate.
- **Tenancy**: every tenant-scoped table has `community_id` and RLS policies keyed on
  `nullif(current_setting('app.community_id', true), '')::uuid`. Handlers take `Tenant`; all
  tenant-scoped queries run inside `TenantTx`, which does `SET LOCAL app.community_id` and
  `SET LOCAL ROLE courtpit_app` (a NOBYPASSRLS, non-owner role) so RLS actually applies.
  Global tables (`users`, `sessions`, `email_codes`, `auth_identities`, `communities`, `jobs`)
  are queried on the pool directly.
- **OpenAPI**: every handler has `#[utoipa::path]` and is registered with `routes!` in
  `app::api_router`, so `/api/v1/openapi.json` stays complete.
- **Lists** use cursor pagination (`?cursor=&limit=`, response `{ items, next_cursor }`).
- **Money**: no money columns in steps 1–3 (only the nullable `leagues.entry_fee_minor` seam).

## Local environment
- Postgres 16: `DATABASE_URL=postgres://courtpit:courtpit@127.0.0.1/courtpit` (see `.env.example`;
  the role must be allowed to create databases for the test harness). `service postgresql start`
  if it is not running.
- Share one target dir across worktrees to reuse compiled deps:
  `export CARGO_TARGET_DIR=/home/claude/courtpit-target`.
- No `sqlx-cli` needed: `cargo run -p courtpit-server -- migrate` applies migrations.

## Tests
- `domain`: plain unit tests (`cargo test -p courtpit-domain`).
- `server`: one integration test binary `crates/server/tests/it/` (a module per area).
  **Per-test database cloned from a migrated template**: the harness migrates a template DB
  named `courtpit_tpl_<hash of migrations>` once (under an advisory lock), then every test runs
  `CREATE DATABASE courtpit_test_<uuid> TEMPLATE ...` — fast and fully isolated. Leftover test
  databases from earlier runs are dropped at harness start. Needs `DATABASE_URL` pointing at a
  role allowed to create databases (default `postgres://courtpit:courtpit@127.0.0.1/courtpit`).
- Use the helpers in `tests/it/common.rs` (`TestApp::spawn`, `app.req(..)`, `app.login(..)`)
  rather than hand-rolling requests.

## Quality gate (every PR, must be green)
```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

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
