# Courtpit

White-label, multi-tenant tennis community app: player directory, friendly-match finder,
seasonal ranked leagues (and later tournaments), self-reported scores confirmed by opponents.

- Architecture: [`docs/architecture.md`](docs/architecture.md)
- Decisions beyond the spec: [`docs/decisions.md`](docs/decisions.md)
- Deploying (Fly.io + Neon + R2): [`docs/deploy.md`](docs/deploy.md)
- Contributor/agent conventions: [`CLAUDE.md`](CLAUDE.md)

## Quick start

```sh
cp .env.example .env            # adjust DATABASE_URL
cargo run -p courtpit-server -- serve
curl localhost:8080/healthz
```

Requires Rust (edition 2024) and Postgres 16. Local default:
`DATABASE_URL=postgres://courtpit:courtpit@127.0.0.1/courtpit` (the role must be able to
`CREATE DATABASE`; integration tests create a throwaway database per test).

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace          # integration tests need DATABASE_URL
(cd packages/api-client && npm ci && npm run check && npm run typecheck)   # TS client vs OpenAPI
```
