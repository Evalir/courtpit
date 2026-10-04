# Deploying Courtpit

Phase 1 (before payments) runs for about nothing a month: one Fly Machine that sleeps when
idle, a Neon Free database, and Cloudflare R2 for off-platform backups. Phase 2 (when payments
ship) swaps the database for an always-on Postgres on Fly; see the last section.

| Piece | Where | Notes |
|---|---|---|
| API (`courtpit-server serve`) | one Fly Machine, `min_machines_running = 0` | Stops when idle, starts on the next request; billed only while running. Cold starts are accepted. |
| Database | Neon Free, **pooled** connection string | Scales to zero after 5 min idle; 1 GB; only 6 h of point-in-time restore, hence the R2 dump. |
| Due jobs (`courtpit-server tick`) | a Fly **scheduled Machine**, hourly | Nothing polls the jobs table while the API Machine is stopped; the tick drains due jobs and exits. |
| Backups | nightly `pg_dump` streamed to Cloudflare R2 | Run as a job by the server/tick; needs `pg_dump` in the image and Neon's *direct* URL. |
| Email | Resend | `COURTPIT_MAILER=resend`. |
| Deploys | GitHub Actions | `.github/workflows/deploy.yml` runs after CI passes on `main`. |

Commands below run from the repository root, so `fly` picks the app up from `fly.toml`.

## 1. Prerequisites

- A Fly.io account with a payment method, and `flyctl` (`curl -L https://fly.io/install.sh | sh`,
  then `fly auth login`). `jq` is used in a few commands.
- A Neon account, a Cloudflare account (R2 needs a payment method on file; the free allowance
  is far above what backups use), a Resend account.
- A domain for the app (these docs say `courtpit.app`) whose DNS you can edit.

## 2. Create the app

App names are global on Fly, so "courtpit" is probably taken; these docs use `my-courtpit`.
Create yours, then edit the two lines marked `CHANGE ME` at the top of `fly.toml` (`app` and
`primary_region`):

```sh
fly apps create my-courtpit
fly platform regions        # pick the region nearest your Neon project, e.g. ams
```

This uses `fly apps create` rather than `fly launch --no-deploy` because `fly launch` rescans
the repository and may rewrite the committed `fly.toml`.

## 3. Database (Neon)

1. Create a Neon project in a region near Fly's. Any Postgres version from 14 to 18 works
   (new Neon projects default to 18). Note the default role `neondb_owner` and database
   `neondb`.
2. In the console's **Connect** dialog turn **Connection pooling** on and copy the string. The
   host contains `-pooler`. Keep `sslmode=require`; drop `channel_binding=require` if present
   (sqlx ignores it and logs a warning on every connection).
3. Store it, staged until the first deploy:

```sh
fly secrets set --stage 'DATABASE_URL=postgresql://neondb_owner:<password>@ep-xxxx-pooler.eu-central-1.aws.neon.tech/neondb?sslmode=require'
```

Things to know:

- **The migrating role.** The first migration creates the `courtpit_app` role (NOLOGIN,
  NOBYPASSRLS) that every tenant-scoped query runs as via `SET LOCAL ROLE`, and grants it to
  the connecting role, so that role needs `CREATEROLE`. Neon's `neondb_owner`, and every role
  created in the console, CLI or API, is a member of `neon_superuser`, which has `CREATEROLE`
  (and `CREATEDB`, `BYPASSRLS`, `REPLICATION`), so the first `migrate` works as is. That
  `BYPASSRLS` does not weaken tenancy: queries drop to `courtpit_app` first. A role created
  with plain SQL `CREATE ROLE` lacks it; either use `neondb_owner` or create the role once,
  as `neondb_owner`, before the first deploy:

  ```sql
  CREATE ROLE courtpit_app NOLOGIN NOSUPERUSER NOBYPASSRLS NOINHERIT;
  GRANT courtpit_app TO <the login role in DATABASE_URL>;
  ```

- **Transaction pooling.** The app only uses transaction-scoped state (`SET LOCAL`,
  `set_config(.., true)`) and protocol-level prepared statements, both of which work through
  Neon's PgBouncer. If you ever see prepared-statement errors, append
  `&statement-cache-capacity=0` to the URL.
- **Migrations go through the pooler for now.** Neon recommends a direct connection for schema
  migrations because sqlx's migrator takes a session-level advisory lock, which transaction
  pooling does not honour. With one deploy at a time (the workflow serialises them) this is
  fine in practice; a separate direct URL for `migrate` is planned. If a release ever hangs
  waiting for the lock, run the migration once through the direct URL (needs the
  `BACKUP_DATABASE_URL` from step 4):

  ```sh
  fly ssh console --machine "$(fly machines list --json | jq -r '.[] | select(.config.metadata.fly_process_group == "app") | .id')" \
    -C 'sh -c "DATABASE_URL=$BACKUP_DATABASE_URL courtpit-server migrate"'
  ```

- **Cold starts.** After 5 idle minutes Neon suspends the compute and the next query waits for
  it to wake (typically under a second; the pool waits up to 5 s for a connection).

## 4. Backups (Cloudflare R2)

The nightly job dumps with `pg_dump`, which cannot use transaction pooling, so it needs
Neon's **direct** URL (the same string without `-pooler`).

1. R2 → **Create bucket** (e.g. `courtpit-backups`), private.
2. R2 → **Manage API tokens** → create a token with **Object Read & Write**, scoped to that
   bucket. Copy the access key id and secret (shown once) and your account id (the endpoint is
   `https://<account_id>.r2.cloudflarestorage.com`).
3. Set the secrets:

```sh
fly secrets set --stage \
  BACKUP_S3_ENDPOINT=https://<account_id>.r2.cloudflarestorage.com \
  BACKUP_S3_BUCKET=courtpit-backups \
  BACKUP_S3_ACCESS_KEY=<access key id> \
  BACKUP_S3_SECRET_KEY=<secret access key> \
  'BACKUP_DATABASE_URL=postgresql://neondb_owner:<password>@ep-xxxx.eu-central-1.aws.neon.tech/neondb?sslmode=require'
```

Optional overrides (set them with `fly secrets set` too, not in `fly.toml`: the tick Machine
only inherits `COURTPIT_*` settings from `fly.toml`, and these must match on every Machine):

| Variable | Default |
|---|---|
| `BACKUP_S3_REGION` | `auto` |
| `BACKUP_S3_PREFIX` | `courtpit/` |
| `BACKUP_RETENTION_DAYS` | `14` |
| `BACKUP_HOUR_UTC` | `3` |

## 5. Email (Resend)

Add and verify your sending domain in Resend (it lists the SPF/DKIM records to create), then
create an API key with sending access:

```sh
fly secrets set --stage RESEND_API_KEY=re_xxxx 'COURTPIT_EMAIL_FROM=Courtpit <no-reply@courtpit.app>'
```

`COURTPIT_MAILER=resend` is already set in `fly.toml`; the server refuses to start in that
mode without the key. Optionally enable Sign in with Apple/Google by listing the accepted
audiences (comma-separated; empty disables a provider):

```sh
fly secrets set --stage COURTPIT_APPLE_CLIENT_IDS=app.example.club COURTPIT_GOOGLE_CLIENT_IDS=1234.apps.googleusercontent.com
```

## 6. First deploy

```sh
fly deploy --remote-only --ha=false
```

Fly builds the `Dockerfile` on its remote builder, runs `courtpit-server migrate` in a
temporary Machine (the release command; a failure aborts the deploy), then creates the one web
Machine (`--ha=false` stops Fly adding a spare) and allocates IP addresses. Check:

```sh
fly status
fly ips list                                  # a shared v4 and a v6; if empty: fly ips allocate-v4 --shared && fly ips allocate-v6
curl https://my-courtpit.fly.dev/healthz       # {"status":"ok"}
curl https://my-courtpit.fly.dev/readyz        # {"status":"ok"}, wakes Neon: proves DATABASE_URL works
fly logs                                       # JSON lines
```

### Create the first community

`create-community` talks to the database directly. `fly ssh console` starts the Machine when
you name it with `--machine`:

```sh
WEB=$(fly machines list --json | jq -r '.[] | select(.config.metadata.fly_process_group == "app") | .id')
fly ssh console --machine "$WEB" -C "courtpit-server create-community --slug demo --name 'Demo Club' --owner-email you@example.com"
```

Slugs are lowercase letters, digits and dashes; do not use `api` or `www`, which are host names
you will point at the app. The owner signs in with an emailed one-time code. Until DNS is set
up, address a community with its header:

```sh
curl -H 'X-Courtpit-Community: demo' https://my-courtpit.fly.dev/api/v1/tenant
```

## 7. The hourly tick Machine

`courtpit-server tick` drains due jobs and exits (`--max-seconds`, or
`COURTPIT_TICK_MAX_SECONDS`, caps a run at 300 s by default). Fly can start a Machine on an
hourly schedule, but only for Machines created with `fly machine run`; `fly.toml` cannot
express it (its process groups get always-on Machines). Create it once, after the first
deploy, from the image the web Machine runs:

```sh
IMAGE=$(fly machines list --json | jq -r '[.[] | select(.config.metadata.fly_process_group == "app")][0].config.image')
fly machine run \
  --app my-courtpit --region ams \
  --name courtpit-tick \
  --schedule hourly \
  --restart no \
  --vm-size shared-cpu-1x --vm-memory 512 \
  --metadata courtpit_role=tick \
  --env COURTPIT_LOG_FORMAT=json \
  --env COURTPIT_MAILER=resend \
  --env COURTPIT_BASE_DOMAIN=courtpit.app \
  --env COURTPIT_DB_MAX_CONNECTIONS=5 \
  "$IMAGE" courtpit-server tick
```

- It inherits the app's secrets (`DATABASE_URL`, `RESEND_API_KEY`, `BACKUP_*`) automatically
  but **not** `fly.toml`'s `[env]`, hence the `--env` flags; keep them equal to `fly.toml`.
  The deploy workflow copies every `COURTPIT_*` value from the web Machine onto it after each
  deploy.
- `--restart no`: a failed run waits for the next hour instead of looping; interrupted jobs
  are re-claimed once their lease expires.
- Hourly means "every hour from creation", not on the hour. Re-create the Machine to move it.
- `fly deploy` only updates Machines that Fly Launch manages (those carrying the
  `fly_platform_version=v2` metadata), so it leaves this one alone and never touches its
  schedule. The `courtpit_role=tick` metadata is how the deploy workflow finds it, and the
  workflow runs `fly machine update <id> --image <web image> --skip-start --yes`, which keeps
  the schedule and keeps the tick on the exact image of each release. Without such a Machine
  the workflow prints a warning and carries on.

To run a tick right now without waiting, in the web Machine's environment:

```sh
fly ssh console --machine "$WEB" -C "courtpit-server tick"
```

and read the scheduled runs with `fly logs --machine <tick id>`. `fly machines list` shows the
tick `stopped` between runs.

## 8. DNS and certificates

Tenants are reached as `{slug}.courtpit.app` (a wildcard), or on a community's own domain. On
another domain, change `COURTPIT_BASE_DOMAIN` in `fly.toml` (and in the tick command).

```sh
fly certs add courtpit.app            # the apex
fly certs add "*.courtpit.app"        # every community, and api.courtpit.app
fly ips list                          # the addresses to point at
fly certs show "*.courtpit.app"       # prints the DNS records still missing
```

Create, at your DNS provider:

| Type | Name | Value |
|---|---|---|
| A | `courtpit.app` | the shared IPv4 from `fly ips list` |
| AAAA | `courtpit.app` | the IPv6 |
| A / AAAA | `*.courtpit.app` | the same two addresses |
| CNAME | `_acme-challenge.courtpit.app` | exactly what `fly certs show "*.courtpit.app"` prints (looks like `courtpit.app.<id>.flydns.net`) |

A wildcard certificate can only be issued by DNS validation, hence the `_acme-challenge`
CNAME; it must be a CNAME (no TXT record at that name) and, on Cloudflare DNS, not proxied.
`fly certs check "*.courtpit.app"` reports progress. The wildcard does not cover the apex,
which is why both certificates are added. Native apps can use `https://api.courtpit.app`
with the `X-Courtpit-Community: <slug>` header, which takes precedence over the host.

**Per-community custom domains** (e.g. `tennis.example.org`): add the certificate, have the
club point the name at the app (`CNAME` to `my-courtpit.fly.dev`, or `A`/`AAAA` to the same
addresses), and register it on the community:

```sh
fly certs add tennis.example.org
fly certs show tennis.example.org
```

Pass `--custom-domain tennis.example.org` to `create-community` for a new community. For an
existing one there is no admin endpoint yet; run once against the database (Neon's SQL editor
or `psql`; communities are cached for 60 s):

```sql
UPDATE communities SET custom_domain = 'tennis.example.org' WHERE slug = 'demo';
```

## 9. Deploys from GitHub

`.github/workflows/deploy.yml` runs when the **CI** workflow succeeds for a push to `main`
(pull-request CI runs never deploy), and can be started by hand from `main` (Actions → Deploy
→ Run workflow). Runs share one concurrency group, so deploys never overlap and an in-flight
deploy is never cancelled; if `main` has moved on by the time a run starts, it skips itself.
It runs `flyctl deploy --remote-only --ha=false`, then points the tick Machine at the new image.

Do the first deploy by hand (step 6) before enabling it. Then create a deploy token, scoped to
this app, and store it as the repository secret `FLY_API_TOKEN` (keep the whole output,
including the `FlyV1` prefix):

```sh
fly tokens create deploy
gh secret set FLY_API_TOKEN
```

(or Settings → Secrets and variables → Actions → New repository secret).

## 10. The image

`Dockerfile` is multi-stage: cargo-chef caches the dependency build, `cargo build --release`
embeds `migrations/` at compile time, and the runtime is `debian:trixie-slim` with
`ca-certificates`, a non-root user (uid 10001), the binary at `/usr/local/bin/courtpit-server`
and `postgresql-client-18` from the PostgreSQL (PGDG) apt repository. `CMD` is
`courtpit-server serve` and there is no `ENTRYPOINT`, so `release_command` and the tick
Machine replace the command cleanly.

`pg_dump` must be at least as new as the server. Neon creates Postgres 18 projects by default
and supports 14 to 18; Debian trixie ships only 17 (bookworm 15), so the client comes from
PGDG, and a newer client dumps every older server. When Neon moves its default past 18, bump
`PG_MAJOR` in the `Dockerfile`.

```sh
docker build -t courtpit .
docker run --rm courtpit courtpit-server --help
docker run --rm courtpit pg_dump --version
```

## Phase 2: payments

When payments ship, move the database to an always-on Postgres on Fly, and keep the app
awake:

- `fly.toml`: `min_machines_running = 1` (and `auto_stop_machines = "off"`); the job loop then
  runs continuously, so the tick Machine and its workflow step can go
  (`fly machine destroy <id>`).
- `DATABASE_URL` becomes the Postgres app's internal address; PgBouncer stops mattering for
  migrations and the pooled/direct split disappears. Keep `BACKUP_*` pointed at the new
  database: the nightly dump to R2 continues unchanged.
- Nothing in the schema or the server changes between phases.
