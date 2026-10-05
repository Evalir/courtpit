# Racquet Collective

Spec: `docs/architecture.md` (authoritative). Where it is silent, decide, and record the
decision in `docs/decisions.md`. Read the relevant spec section before changing behaviour.

## Commands

`<crate>` is `racquetcollective-domain` or `racquetcollective-server`.

- `cargo +nightly fmt --all` - format
- `cargo clippy -p <crate> --all-features --all-targets` - lint with features
- `cargo clippy -p <crate> --no-default-features --all-targets` - lint without
- `cargo t -p <crate>` - test specific crate (the server's tests need Postgres, see
  [Local environment](#local-environment))

Always lint with both feature sets. Never use `cargo check` or `cargo build`;
clippy covers them. Run affected crates first; broaden for cross-crate changes.

### Pre-push Checks (enforced by Claude hook)

`.claude/settings.json` runs `.claude/hooks/pre-push.sh` before every
`git push` (new commits, rebases, cherry-picks). The push is blocked if any
check fails:

- `cargo +nightly fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo clippy --workspace --all-targets --no-default-features -- -D warnings`
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items`

Clippy and doc warnings are hard failures.

## Design: types first

The type graph is the architecture. Someone reading only the public types,
their fields, and the signatures connecting them should be able to redraw the
system diagram: which components exist, what each owns, what states it moves
through, and how data flows between them. Design types before writing logic;
when a change doesn't fit the existing types, change the types rather than
adding procedural glue.

### Behavior lives on the type that owns the invariant

- An operation with a clear receiver is a method. Constructors are associated
  fns (`new`, `with_*`, `from_*`) and go at the top of the `impl`.
- Conversions go on the most specific type, via `From`/`TryFrom`/`AsRef`/
  `FromStr`, or `as_`/`to_`/`into_` methods. Implement `FromStr` for `X`
  rather than writing a public `fn parse_x(&str) -> X`.
- Don't fake a receiver. A function that doesn't use `self` is an associated
  fn or a free fn.
- Public free fns are allowed only when there's no single natural receiver:
  - symmetric operations over several types;
  - process-wide setup (metrics, tracing subscriber);
  - composition roots that wire components together;
  - CLI command entry points;
  - pure transformation passes over a data structure.

  A free fn whose first argument is always the same type is a method in
  disguise.
- Private module-local helper fns are fine; privacy is per module.

### Encode invariants in types (parse, don't validate)

- Validate once at the boundary and carry a type that proves it
  (`ValidatedTx`, `NonZeroU64`). Downstream code never re-checks.
- Newtype every identifier or quantity that could be confused with another
  of the same representation (`BlockNumber(u64)`, `Wei(U256)`, typed arena
  indices). Never pass two bare `u64`s with different meanings.
- Make illegal states unrepresentable: enums for alternatives, not `Option`
  fields plus `bool` flags that must agree.
- No `bool`/`Option` params that select behavior; use an enum
  (`fetch(Mode::Cached)`, not `fetch(true)`).
- A struct is either plain data (all fields `pub`, no invariants) or an
  abstraction (all fields private, constructors enforce invariants). Never mix.
- Multi-step lifecycles where out-of-order calls are bugs use typestate,
  consuming `self` on each transition (e.g. `Evm<NeedsCfg>` → `Evm<Ready>` →
  `Evm<Transacted>`). For internal state callers never drive, a private enum
  is enough.
- Use builders for structs with >4 fields or several fields of the same type;
  `build()` returns the validated type.

### Make ownership and topology visible

- The ownership tree is the component tree. Prefer owned fields and a clear
  parent → child structure. Use `Arc<Mutex<_>>` only for real shared mutable
  state, wrapped in a named type that explains why it's shared.
- For graph-shaped data, use arenas with typed indices (`IndexVec<Id, T>`), not
  `Rc<RefCell<_>>` webs. Do index arithmetic on the index type rather than
  `.index()` round-trips. Use bitsets, never `Vec<bool>`.
- Long-lived tasks are types: a `FooService` owns its state and exposes
  `spawn(self) -> JoinHandle<_>` (or returns a spawnable future). Never run a
  long-lived task directly. Callers talk to it through a cloneable
  `FooHandle` that wraps the channel senders with typed methods. Never pass
  raw `mpsc::Sender` across component boundaries.
- Messages between components are enums named for the protocol
  (`EngineCommand`, `PoolEvent`).

### Traits are for real polymorphism

- Add a trait only for ≥2 real implementations or a documented extension
  point. No "interface for one impl". Public traits include an
  implementation guide in their docs.
- Implement std traits instead of look-alikes: `Display`, `Default`,
  `IntoIterator` alongside `iter()`, `FromIterator`, `Extend`.
- No `Deref`-as-inheritance. Use composition and explicit delegation.
- Keep generics bounded to what's needed. At user-facing boundaries, minimize
  generics and provide concrete types or aliases.

### Document the exploded view

- Each crate's `//!` docs include an **Architecture** section naming the core
  types and their relationships with intra-doc links. Broken links fail the
  doc check, so the map can't drift. Update it in the same change as the code.

## Code style

### Control flow

- Functional combinators over imperative control flow. No unnecessary nesting.
- `let Some(x) = x else { return };` for a single early-exit guard.
- Several conditions gating one block: a single `if let` chain
  (`if let Some(x) = a && let Some(y) = b`), not nesting or stacked `let ... else`.
  In loops, wrap the body in an `if let` chain rather than repeated
  `let ... else { continue };`.
- Don't use `ref`/`ref mut` as a first resort; borrow the scrutinee with `&`/`&mut`.
- Use map entry APIs instead of lookup-then-insert.
- Rely on inference. When a type is needed, use turbofish
  (`Type::<X>::new()`) over `let x: Type<X> = ...`.
- Small, focused functions and types. Never add incomplete code or `TODO`s
  for core logic.

### Items and docs

- New items go at the bottom of their scope or group; match the file's
  existing order. The file's primary type comes before supporting types and
  private helpers.
- Doc comments come before attributes (`///` then `#[derive]`).
- Module docs are `//!` at the top of the module file, not on the `mod` item.
- Document all public items with concise usage examples; hide scaffolding
  with `#`. Feature-gated items get
  `#[cfg_attr(docsrs, doc(cfg(feature = "...")))]`.
- Leave a blank line between items, except for a one-off struct with a single
  impl or a run of near-identical impls.
- Comments end with periods (except URLs). Explain non-obvious behavior in
  comments, not PR history. Every `unsafe` block has a `// SAFETY:` comment.

### Imports and visibility

- All imports at the top of the file; never inside functions unless needed
  for `#[cfg]` gating.
- One crate per `use` statement (rustfmt enforces this). No glob imports,
  except `use super::*` in test modules and `prelude::*`.
- Plain `use` imports form one block with no blank lines inside it.
  Separate blocks, divided by one blank line, only for: `pub use` re-exports
  (with `mod x;` before `pub use x::...;`), and each distinct `#[cfg(...)]`
  condition, which comes after the unconditional imports.
- Test-only imports go inside the `#[cfg(test)]` module.
- Private by default, `pub(crate)` for internal, `pub` for API only. Never
  `pub(super)`.

### Errors

- `thiserror` in libraries; `eyre` only in binaries; never `anyhow`.
- Propagate with `?` and `map_err`. A function that can't fail doesn't
  return `Result`.
- Diagnostic and error messages: no trailing full stop; code in backticks.

### Tracing and async

- Use `tracing`. Instrument work items, not long-lived tasks.
  `#[instrument(skip(self))]` on methods; record only the fields you need.
- Levels: TRACE (rare, verbose), DEBUG (sparingly), INFO (default),
  WARN (potential issues), ERROR (prevents operation). Don't prefix messages
  with the function name; the span provides it.
- Propagate spans across task boundaries with `.instrument(span)`.
- Tokio conventions. Never block in async code; keep blocking work off
  executor threads.
- Hot paths: avoid unnecessary allocations, reuse buffers, borrow.

### Testing

- Tests panic and never return `Result`; use `unwrap()`.
- Unit tests in `mod tests` at the bottom of the file, starting with
  `use super::*;`. Integration tests in `tests/`.
- Use fuzz tests for parsing/serialization and property tests for invariants.
- Where snapshot testing exists, assert exact output rather than `.contains`.
- In integration tests, wait on conditions instead of sleeping.

### Semver

- A dependency that appears in the public API (re-exported, or in public
  fields, signatures or trait impls) makes a semver-incompatible bump of it
  breaking for us too. Pre-1.0, bump our minor version.

## Git and GitHub

- Fresh branches off `main` with descriptive names.
- Conventional commits and PR titles: `type(scope): description`
  (`feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `chore`).
- PR body: what changed and why in 1–3 sentences, the linked issue, and real
  measurements for perf claims. No file lists or filler.
- AI-authored GitHub comments start with a `**[Claude Code]**` header.
- Never expose secrets. Files use LF and end with a newline.

## Crate Notes

- `racquetcollective-domain` (`crates/domain`) — pure logic. **No IO, no tokio, no sqlx**
  (its dependencies are `rust_decimal`, `serde`, `thiserror`, `uuid` and optional `utoipa`).
  Score validation, match state machine, placement/pairings, scoring rules. Exhaustive unit
  tests live next to the code. Its one feature, `openapi` (off by default), derives
  `utoipa::ToSchema` on public types; the server always enables it.
- `racquetcollective-server` (`crates/server`) — axum 0.8 + sqlx 0.8 (Postgres) + utoipa.
  A library plus the `racquetcollective-server` binary (`src/main.rs`) in one manifest, with
  subcommands `serve`, `tick`, `migrate`, `create-community`, `set-app-links`, `openapi`,
  `seed` (demo data; refuses `RACQUETCOLLECTIVE_ENV=production`). Handlers are thin: parse,
  call `domain`, persist. It uses `anyhow` (library and binary) from before the "never
  `anyhow`" rule; don't spread it to new code.

## Repo Notes

### Layout (outside the Rust crates)

- `apps/mobile` — the Expo app (iOS, Android, web); design and conventions in
  `docs/frontend.md`. `packages/api-client` — the generated TypeScript client. Both are npm
  workspaces of the root `package.json` (one root `package-lock.json`; run `npm ci` at the root).
- `migrations/` — sqlx migrations embedded with `sqlx::migrate!()`. **One new migration file per
  PR that touches schema. Never edit a migration that has shipped** (anything below the stack tip).

### Binding conventions
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

### Frontend conventions (`apps/mobile`)
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

### Local environment
- Postgres 16: `DATABASE_URL=postgres://racquetcollective:racquetcollective@127.0.0.1/racquetcollective` (see `.env.example`;
  the role must be allowed to create databases for the test harness). `service postgresql start`
  if it is not running.
- Share one target dir across worktrees to reuse compiled deps:
  `export CARGO_TARGET_DIR=/home/claude/racquetcollective-target`.
- No `sqlx-cli` needed: `cargo run -p racquetcollective-server -- migrate` applies migrations.

### Tests
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

### Quality gate (every PR, must be green)
CI checks only pull requests that are ready for review and target `main` (plus every push to
`main`): drafts and stacked PRs above the bottom get no CI, so run the gate locally before every
push. To check another branch in CI, run the CI workflow on it by hand (Actions → CI → Run
workflow); minutes are scarce, so only when the local gate can't (decision 106). The pre-push
hook runs all of this but `cargo test` (which needs Postgres).
```sh
cargo +nightly fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy --workspace --all-targets --no-default-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
```
PRs touching `apps/mobile` (from `apps/mobile`, after `npm ci` at the root):
```sh
npm run format:check && npm run lint && npm run typecheck && npm test && npm run export:web
```

### Lints
- The strict set lives in `[workspace.lints]` (root `Cargo.toml`, init4's
  spellbook/jay/signet-sdk set merged with the unified conventions above, decision 107) plus
  thresholds in `clippy.toml`; `rustfmt.toml` uses nightly options, so format with
  `cargo +nightly fmt --all`. Highlights: `missing_docs` on every public item,
  `unreachable_pub` (use `pub(crate)`, including in `tests/it`), `unused_results` (bind ignored
  values with `let _ =`), `min_ident_chars` (no `|e|`/`|r|`), `missing_const_for_fn`,
  `mod_module_files` (`foo.rs` + `foo/`, never `foo/mod.rs`), `too_many_lines` (100),
  `rustdoc::all`.
- Each library crate has `#![cfg_attr(not(test), warn(unused_crate_dependencies))]`: add a
  dependency in the PR that first uses it.
- Silence a lint at the narrowest scope with `#[expect(lint, reason = "...")]`; bare
  `#[allow]` is itself a lint error.

### Stacked PRs
- Overrides "fresh branches off `main`" above for multi-PR work: linear stack `cp/01-…`,
  `cp/02-…`; each branch starts at the previous branch's tip; PR N targets branch N-1, PR 01
  targets `main`. Keep PRs focused (~200–600 changed lines, excluding
  `Cargo.lock`).
- Fixing a lower PR: commit on its branch, then `git rebase --onto <new-base> <old-base> <branch>`
  for every branch above it.
- Every commit message ends with the two trailer lines (with the authoring model and session):
  ```
  Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01DpAja1DpzgBswgjV7HmpwM
  ```
