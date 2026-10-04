# Courtpit

White-label, multi-tenant tennis community app: player directory, friendly-match finder,
seasonal ranked leagues (and later tournaments), self-reported scores confirmed by opponents.

- Architecture: [`docs/architecture.md`](docs/architecture.md)
- Decisions beyond the spec: [`docs/decisions.md`](docs/decisions.md)
- Deploying (Fly.io + Neon + R2): [`docs/deploy.md`](docs/deploy.md)
- The app (Expo; iOS, Android, web): [`docs/frontend.md`](docs/frontend.md)
- Contributor/agent conventions: [`CLAUDE.md`](CLAUDE.md)

## Quick start

Requires Rust (edition 2024, 1.88+) and Postgres 16. Node 22 is needed for the app
(`apps/mobile`, design in [`docs/frontend.md`](docs/frontend.md)) and the TypeScript client in
`packages/api-client`.

```sh
# 1. A local Postgres role that may create databases (the integration tests make one per test)
#    and roles (the first migration creates the RLS-bound `courtpit_app` role).
sudo -u postgres psql -c "CREATE ROLE courtpit LOGIN PASSWORD 'courtpit' CREATEDB CREATEROLE"
sudo -u postgres createdb -O courtpit courtpit

# 2. Configuration comes from the environment (clap reads it; `.env` is not loaded for you).
cp .env.example .env            # defaults: DATABASE_URL=postgres://courtpit:courtpit@127.0.0.1/courtpit
set -a; . ./.env; set +a
export COURTPIT_COOKIE_SECURE=false   # plain-HTTP localhost

# 3. Schema, demo data, server.
cargo run -p courtpit-server -- migrate
cargo run -p courtpit-server -- seed     # demo community `demo` (`--slug` to change); refused when COURTPIT_ENV=production
cargo run -p courtpit-server -- serve    # API on :8080, background job loop included
curl localhost:8080/healthz
curl -H 'X-Courtpit-Community: demo' localhost:8080/api/v1/tenant
```

Sign in as the owner `seed` prints (`marcus.hale@example.com`); with `COURTPIT_MAILER=log` the
emailed code appears in the server log. Re-running `seed` is safe: it updates in place.

The app, against that server:

```sh
npm ci                        # repository root: npm workspaces (apps/mobile, packages/api-client)
cd apps/mobile && npm run web # http://localhost:8081; /api is proxied to localhost:8080
```

`lily.fernandez@example.com` is a seeded player with a league score waiting for her confirmation.

Other subcommands:

| Command | What it does |
|---|---|
| `tick [--max-seconds 300]` | Runs due background jobs once and exits — what the hourly Fly scheduled Machine runs while the API sleeps. Also schedules the nightly backup when `BACKUP_S3_*` is set. |
| `create-community --slug .. --name .. [--owner-email ..]` | Adds a community (tenant). |
| `openapi [--out file]` | Prints the OpenAPI document; needs no database. |

Deploying (Fly.io + Neon + Cloudflare R2): [`docs/deploy.md`](docs/deploy.md).

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace          # integration tests need DATABASE_URL
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
npm ci                                                       # once, at the root
(cd packages/api-client && npm run check && npm run typecheck)   # TS client vs OpenAPI
(cd apps/mobile && npm run format:check && npm run lint && npm run typecheck && npm test)
```

CI also runs the whole test suite through pgbouncer in transaction mode (how production reaches
Neon); to do the same locally, see "Running the pooled test suite locally" in
[`docs/deploy.md`](docs/deploy.md).
