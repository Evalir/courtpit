# Racquet Collective — Architecture

**Status:** v0.1 draft, agreed in discussion on 2026-10-02; updated 2026-10-03 with the Phase 1/Phase 2 hosting plan (§17) and the resolved open questions (§20). Living document; the Decisions Log at the end records what was settled and why, and `docs/decisions.md` records the finer rulings.

## 1. Product scope (v1)

Racquet Collective is a white-label, multi-tenant tennis community app. Each *community* (a club, a group, a city scene) gets its own branded app in which players sign up, fill in a profile, find friendly matches by level, and compete in seasonal ranked leagues and one-off tournaments. Match scores are self-reported and confirmed by the opponent. Players accumulate community ranking points from league and tournament play.

v1 includes singles, doubles and mixed doubles in leagues and tournaments; open sign-up; entry fees paid in-app via Stripe with a switchable Racquet Collective platform fee; push notifications; and a native mobile client with a web surface as a by-product. One community is live at launch, but every layer is tenant-aware from day one.

## 2. Principles

Minimal moving parts: one Rust binary, one Postgres database, and three external HTTP services (Stripe, an email API, Expo push). No queue, no cache server, no second datastore. The service is stateless so scale-out is "run more copies".

Correctness lives in a pure domain crate. Set-score validation, match state transitions, draw and pairing generation, and scoring rules are IO-free Rust functions with unit tests. The HTTP and database layers are deliberately boring.

Tenant scoping is enforced twice: in the type system (an axum extractor every handler must take) and in Postgres (row-level security). A missed `WHERE` clause becomes an empty result, not a data leak.

Rules that communities will want to tune (match formats, scoring, fees) are data, not code.

## 3. System overview

```
 Expo app (iOS / Android / web) ──HTTPS──▶  racquetcollective-server (axum)  ──▶ Postgres
                                                 │   │   │
                                     Stripe ◀────┘   │   └────▶ Expo Push (APNs/FCM)
                                   (Connect,         └────▶ Email API (Resend/Postmark)
                                    Checkout,
                                    webhooks)
```

`racquetcollective-server` serves the REST API, verifies Stripe webhooks, and runs an in-process job loop (season transitions, auto-confirmations, reminders, leaderboard refresh) backed by a `jobs` table. Multiple instances coordinate through Postgres (`FOR UPDATE SKIP LOCKED`), nothing else. While the server is scaled to zero, an hourly `racquetcollective-server tick` drains due jobs and exits (§17).

Expected load is low; the stack is chosen so that a single small instance handles thousands of requests per second on CRUD traffic and the only resource to watch is the Postgres connection pool.

## 4. Repository layout

```
racquetcollective/
  Cargo.toml                 # workspace
  crates/
    domain/                  # pure logic: scores, state machines, draws, pairings, scoring
    server/                  # axum app, auth, extractors, sqlx queries, migrations, jobs
                             # bin subcommands: serve | tick | migrate | create-community |
                             #   seed | openapi (later: set-fee ...)
  apps/
    mobile/                  # Expo + Expo Router + react-native-web; EAS build profiles per community
  packages/
    api-client/              # TypeScript client generated from the server's OpenAPI spec
                             #   (openapi-typescript + openapi-fetch; committed, drift-checked in CI)
  migrations/                # sqlx migrations
```

Two crates is intentional. Split `server` further only when a module is clearly reusable (e.g. a thin Stripe client).

Core dependencies: `axum`, `tokio`, `sqlx` (Postgres; runtime-checked queries, see `docs/decisions.md` #1), `utoipa` (OpenAPI), `argon2`, `reqwest`, `tracing` + OpenTelemetry, `moka` (in-memory tenant cache). Stripe via a thin hand-written `reqwest` client for the handful of endpoints needed, rather than the full `async-stripe` crate.

## 5. Tenancy and white-labelling

`communities` is the tenant table. Every tenant-scoped table carries `community_id`.

Tenant resolution: for web, from the request host (`{slug}.racquetcollective.app` or a registered custom domain) via a cached lookup; for native, the community slug is baked into the per-community build and sent as a header. In both cases the resolved tenant is cross-checked against the authenticated user's membership before any scoped query runs.

Enforcement: a `Tenant` extractor that handlers must accept, plus Postgres RLS policies on every scoped table keyed on `current_setting('app.community_id')`, set with `SET LOCAL` at the start of each transaction.

Branding is a JSONB column on `communities`: display name, logo URL, color tokens, typography choice, feature flags. The client fetches `GET /api/v1/tenant` at boot and applies the theme at runtime. Native builds bake in identity (bundle id, app name, icon, slug) but not colors, so a rebrand does not need a store release.

## 6. Identity, auth and sessions

Identity and membership are separate. `users` is the global login identity; `players` is a user's membership in one community and carries the whole tennis profile. This allows one person in several communities, and lets a community owner also be a player, without migrations.

Authentication methods: email + 6-digit one-time code (primary — magic links are fragile when the target is a native app), optional password (argon2id), Sign in with Apple, and Google sign-in. Apple sign-in is mandatory on iOS once any other third-party login is offered. Email must be verified before a player appears in the directory or can register for anything.

Sessions are opaque random tokens, stored hashed in `sessions` with an expiry and device label; delivered as an httpOnly cookie on web and a bearer token on native. Revocation is a row delete. No JWTs.

## 7. Domain model

Schema sketch (types abbreviated; all scoped tables also have `community_id`, `created_at`, `updated_at`):

```sql
users            (id, email unique, email_verified_at, password_hash null)
auth_identities  (user_id, provider enum(apple,google), subject, unique(provider,subject))
sessions         (token_hash pk, user_id, expires_at, device_label)
email_codes      (user_id, code_hash, purpose enum(verify,login), expires_at, consumed_at)

communities      (id, slug unique, name, custom_domain null, branding jsonb, settings jsonb,
                  join_policy enum(open), scoring_config jsonb, default_match_format jsonb,
                  currency, stripe_account_id null, stripe_onboarding_complete bool,
                  platform_fee_bps int, platform_fee_fixed_minor int)

players          (id, community_id, user_id, unique(community_id,user_id),
                  display_name, utr numeric(4,2),
                  gender enum(female,male,other,undisclosed),      -- needed for mixed eligibility
                  phone, phone_visible bool, socials jsonb, socials_visible bool,
                  racket, strings, tension_kg numeric, play_pref enum(singles,doubles,any),
                  preferred_locations jsonb, role enum(player,admin,owner),
                  status enum(active,banned))

leagues          (id, community_id, name, discipline enum(singles,doubles,mixed),
                  registration_opens_at, registration_closes_at, starts_at, ends_at,
                  status enum(draft,registration,active,finished,cancelled),
                  match_format jsonb, entry_fee_minor int null, scoring_overrides jsonb null)
league_divisions (id, league_id, name, tier int, utr_min, utr_max)
league_entries   (id, league_id, division_id null, player_ids uuid[], created_by,
                  status enum(pending_partner,pending_payment,confirmed,withdrawn),
                  looking_for_partner bool)

tournaments      (id, community_id, name, discipline, format enum(single_elim,round_robin),
                  draw_size int, registration_opens_at, registration_closes_at, starts_at, ends_at,
                  status, match_format jsonb, entry_fee_minor int null, round_deadline_days int)
tournament_entries (id, tournament_id, player_ids uuid[], created_by, status, seed int null,
                  looking_for_partner bool)

matches          (id, community_id, discipline,
                  league_id null, division_id null, tournament_id null, round int null,
                  side_a_players uuid[], side_b_players uuid[],
                  status enum(proposed,scheduled,reported,confirmed,disputed,resolved,
                              walkover,cancelled),
                  scheduled_at null, location null, score jsonb null, winner_side enum(a,b) null,
                  reported_by null, reported_at null, confirm_deadline_at null,
                  resolved_by null, resolution_note null)
match_proposals  (id, match_id, proposed_by, proposed_time, location,
                  status enum(open,accepted,declined))

match_requests   (id, community_id, created_by, discipline, slots_open int,
                  utr_min, utr_max, time_window_start, time_window_end, location,
                  status enum(open,filled,cancelled))
match_request_joins (request_id, player_id)

ranking_events   (id, community_id, player_id, discipline,
                  source enum(league_match,league_season,tournament), source_id,
                  points int, occurred_at)
rankings         (community_id, player_id, discipline, points_52w, rank)   -- materialised by job

payments         (id, community_id, player_id, entry_kind enum(league,tournament), entry_id,
                  stripe_checkout_session_id, stripe_payment_intent_id null,
                  amount_minor int, currency, platform_fee_minor int,
                  status enum(pending,succeeded,refunded,failed))
stripe_events    (id text pk, type, received_at, processed_at null)

device_tokens    (id, user_id, expo_push_token unique, platform, last_seen_at)
notification_prefs (player_id pk, match_updates bool, league_updates bool, reminders bool)
jobs             (id, kind, payload jsonb, run_at, locked_at null, attempts int, last_error null)
```

Entries, not players, are the unit of participation in a league or tournament. `player_ids` has one element for singles and two for doubles/mixed. Matches copy the two entries' players into `side_a_players` / `side_b_players`, so league, tournament and friendly matches share one table and one state machine.

Gender is collected on the profile as an optional field with `undisclosed` as default. It exists only to validate mixed-doubles eligibility; it is never shown in the directory.

## 8. Matches

State machine (in `domain`):

```
   ┌───────────────── report ─────────────────┐
   │                                          ▼
proposed ──accept──▶ scheduled ──report──▶ reported ──confirm / timeout──▶ confirmed
   │                     │                    │
   │                     │                    └──dispute──▶ disputed ──admin──▶ resolved
   │                     └──no-show / admin──▶ walkover
   └──decline all / admin──▶ cancelled
```

Reporting a score implies the match was played, so a score may be reported straight from `proposed` (any open proposal is then superseded); league matches, which start `proposed`, are often played this way.

Scheduling is a proposal flow: either side proposes a time and place, the other accepts or counter-proposes. No chat in v1; polling is sufficient.

Score reporting: any player on either side submits the score as a list of sets. Anyone on the other side can confirm or dispute within `confirm_window_days` (default 3); unanswered reports auto-confirm by job. Disputes are resolved by a community admin, who may set the score, order a replay, or void the match.

Score format and validation:

```json
{ "sets": [ {"a": 6, "b": 4}, {"a": 3, "b": 6}, {"a": 10, "b": 7, "match_tiebreak": true} ] }
```

`match_format` controls what is legal, e.g. `{"sets_to_win": 2, "games_per_set": 6, "tiebreak_at": 6, "final_set": "match_tiebreak_10"}` (alternatives: `"full_set"`, `"pro_set_8"`). An optional `"deuce"` is `"advantage"` (default) or `"golden_point"` (no-ad: the point at 40–40 decides the game); since scores are reported per set it is informational and does not change which scores are legal. The domain crate checks each set (win by two, 7–5, 7–6 when a tiebreak applies, 10-point match tiebreak when configured) and derives `winner_side`. Communities set a default format; leagues and tournaments can override it.

## 9. Leagues

Lifecycle `draft → registration → active → finished` is driven by the dates; a job flips status on time. Admins can cancel at any stage (triggering refunds).

Registration: a player creates an entry. For singles it is immediately `confirmed` (or `pending_payment` if there is a fee). For doubles and mixed the creator names a partner; the entry is `pending_partner` until the partner accepts, and each partner pays their own share on acceptance. A `looking_for_partner` flag makes solo registrants visible to each other; admins can pair remaining solos before the draw. By default mixed entries require exactly one `female` and one `male` player, and `undisclosed` or `other` makes a player ineligible for mixed divisions (the UI says why). A community can set `settings.mixed_eligibility` to `"any_two_distinct"` to admit any two different disclosed genders, so `other` players can enter mixed; `undisclosed` is never eligible. Friendly mixed matches never check gender.

Divisions ("boxes") are UTR bands of 6–8 entries with a `tier` (1 = top), always generated by placement (there is no manual division mode). At `registration → active`, the domain crate places confirmed entries into divisions and generates a full round-robin schedule per division (circle method). Entries then self-schedule their matches within the active window. A league that reaches `starts_at` with fewer than two confirmed entries (or a box under two) is cancelled instead, and its entrants are emailed.

Standings per division are computed from confirmed matches using the league scoring rules. At `ends_at` unplayed matches are cancelled, but the season only finishes once no league match is still `reported` or `disputed`: reported scores auto-confirm, admins resolve disputes (listed at `GET /admin/leagues/{id}/unresolved`) or force the finish, which leaves those matches out of the table. At `active → finished`, season-end ranking points are written to the ledger and promotion/relegation is applied for the next season (default: top two up, bottom two down), which is the "ranking moves up or down" in the product description — distinct from the points leaderboard.

## 10. Tournaments

Formats: single elimination (seeded by community ranking, then UTR; byes padded to the next power of two) and round robin for small draws. Draw generation is in `domain`. Each round has a deadline (`round_deadline_days`); unplayed matches at the deadline are decided by admin walkover. Entries follow the same partner and payment rules as leagues. Round-reached points are written to the ledger on completion.

## 11. Friendly match finder

Two surfaces. The player directory lists verified players filterable by UTR band, play preference, and preferred location; a player can propose a friendly match directly, which creates a `match` with no league or tournament. Match requests are open calls — "doubles, Saturday morning, UTR 4–6, two slots open" — that others join; when `slots_open` reaches zero the request is filled and a match is created. Friendly matches are confirmed like any other but write no ranking points.

## 12. Scoring and ranking

The community ranking is defined as a ledger, not a computed field. Every scoring event appends a `ranking_events` row; a player's ranking in a discipline is the rolling 52-week sum of their events (ATP style), materialised into `rankings` by a job. Points decay naturally, every point is auditable, and a rule change is a ledger replay rather than a data fix.

Default `scoring_config` (per community, overridable per league):

```json
{
  "league_match": { "win_straight": 3, "win_deciding": 2, "loss_deciding": 1, "loss_straight": 0,
                    "walkover_win": 2, "walkover_loss": 0 },
  "league_season": { "position_points": [100, 70, 50, 35, 25, 15, 10, 5],
                     "tier_multiplier": { "1": 1.0, "2": 0.7, "3": 0.5, "4": 0.35 } },
  "tournament": { "round_points_pct": { "winner": 100, "final": 60, "semi": 36, "quarter": 18,
                                        "r16": 9, "r32": 4 },
                  "base_by_draw_size": { "8": 100, "16": 150, "32": 250 } },
  "mixed_pooling": "separate"
}
```

Disciplines are ranked separately (singles, doubles, mixed) with a combined view available in the UI. `mixed_pooling` may be set to `"doubles"` to fold mixed points into the doubles ranking for communities that prefer one doubles ladder. In doubles and mixed, each partner receives the full points for the pair's result.

UTR is the self-declared level used for matchmaking and division placement. It never mixes with the points ranking.

## 13. Payments (Stripe Connect)

Money belongs to the community owner, not Racquet Collective. Each community onboards a Stripe Connect Express account during setup (hosted onboarding; Stripe handles KYC). Entry fees are destination charges to the owner's account via Stripe Checkout (hosted page in an in-app browser), so Racquet Collective never holds funds and stays out of PCI scope.

Platform fee: `platform_fee_bps` and `platform_fee_fixed_minor` live on `communities` (with a global default in server config) and may be changed at any time by an admin endpoint or the `set-fee` CLI subcommand, including to zero. The fee is computed per charge at Checkout Session creation and passed as `application_fee_amount`, and the applied amount is recorded on the `payments` row. Changing the fee therefore affects future charges only and leaves a full audit trail.

Flow: creating or accepting an entry with a fee opens a Checkout Session and sets the entry to `pending_payment`; the `checkout.session.completed` webhook marks the payment `succeeded` and advances the entry (to `confirmed`, or to waiting on the other partner). Webhooks are verified by signature, stored in `stripe_events` for idempotency, and processed by the job loop so retries are safe. Fees are charged per player, including in doubles. Refunds are an admin action (withdrawal before the draw, cancelled event) that calls Stripe and transitions the payment to `refunded`.

Apple's in-app-purchase rules do not apply: fees for a physical tennis league fall under the "goods and services consumed outside the app" exemption, so Stripe in the app is permitted.

Subscription billing of community owners by Racquet Collective, if ever wanted, is a separate concern (Stripe Billing on the platform account) and does not touch this design.

## 14. Notifications and background jobs

A tokio task in the server polls `jobs` with `FOR UPDATE SKIP LOCKED`, so several instances never double-fire. Job kinds: league/tournament status transitions, auto-confirm of unanswered scores, round-deadline walkovers, match reminders, leaderboard refresh, Stripe event processing, push and email delivery. Failed jobs retry with backoff and surface `last_error`.

Push (Expo's push service over APNs/FCM) is the primary channel, keyed by `device_tokens`, with email as fallback and for account flows (verification codes, receipts). `notification_prefs` gates non-essential categories. Deep links (universal links / app links) target match, partner invite and payment-return screens.

## 15. API

REST JSON under `/api/v1`, OpenAPI generated by `utoipa` (also printed by `racquetcollective-server openapi`), TypeScript client generated into `packages/api-client` with `npm run gen`, committed, and checked for drift in CI. Resource groups: `tenant`, `auth`, `me`, `players`, `leagues` (+ divisions, entries, standings), `tournaments` (+ entries, draw), `matches` (+ proposals, score, confirm, dispute), `match-requests`, `rankings`, `payments` (checkout, webhook), `admin/*`. Cursor pagination on lists. No GraphQL and no realtime in v1.

## 16. Mobile client and distribution

Expo with Expo Router; `react-native-web` provides the web surface from the same codebase. Web is secondary but required: it is where email flows, Stripe return URLs and shared links land. EAS Update delivers JS-only fixes over the air without store review.

Distribution is one store listing per community, built from a per-community EAS build profile (bundle id, app name, icon, baked slug). Apple's guideline 4.3 (template apps) means these listings should be published under each community's own Apple Developer account rather than a single Racquet Collective account; this requires each community to hold a developer account (and a D-U-N-S number for organisations). With one community at launch this is a single enrolment; the process for onboarding further communities is deferred.

No avatar uploads in v1 (avoids object storage); community logos are URLs in the branding config.

## 17. Deployment, operations, observability

One Docker image built from a multi-stage `Dockerfile`: a Debian slim runtime with `postgresql-client` (for `pg_dump`) rather than distroless. GitHub Actions deploys it to Fly.io after CI passes on `main`; migrations run as Fly's release step (`racquetcollective-server migrate`, over a direct database connection); secrets live in Fly's secret store. `tracing` emits JSON logs with the request id; OpenTelemetry export to a hosted backend's free tier comes later. Integration tests run against a real Postgres service container, and CI runs the whole suite again through pgbouncer in transaction mode before each deploy; the domain crate is covered by plain unit tests. Step-by-step setup is in `docs/deploy.md`.

**Phase 1 (before payments, about $0/month).**

- *Server:* one Fly Machine with `auto_stop_machines = "stop"` and `min_machines_running = 0`, billed only while serving traffic. Cold starts are accepted.
- *Database:* Neon Free (scale-to-zero after 5 minutes, 1 GB, 6 h point-in-time restore). The app connects through Neon's **pooled** connection string (pgbouncer, transaction mode). That is safe because the server keeps no session state: `TenantTx` uses `SET LOCAL`/`set_config(.., true)`, job claims are single `SKIP LOCKED` statements, and Neon's pooler tracks protocol-level prepared statements. Anything that needs a session — migrations (sqlx's advisory lock) and `pg_dump` — uses the direct URL (`DATABASE_DIRECT_URL`).
- *Jobs:* the in-process loop runs while the Machine is awake. Because nothing polls while it is stopped, a Fly **scheduled Machine** runs `racquetcollective-server tick` hourly: it drains due jobs (bounded by `--max-seconds`) and exits. Deadlines such as auto-confirmation and league dates therefore fire within the hour even when nobody uses the app.
- *Backups:* besides Neon's PITR, a nightly `backup_database` job streams `pg_dump --format=custom` to Cloudflare R2 (S3-compatible, free tier) through a small built-in S3 client, and prunes dumps past `BACKUP_RETENTION_DAYS`.
- *Rate limits* stay in memory, which is exact with a single Machine.

**Phase 2 (when payments ship).** Stripe webhooks and checkout returns want a warm, always reachable server, so the database moves to an always-on Postgres on Fly and the app runs with `min_machines_running = 1` (the tick Machine can then go). The pooled/direct split disappears (`RACQUETCOLLECTIVE_DB_POOLED=false`). Nothing in the schema or the server changes between phases.

## 18. Privacy and compliance

Phone and socials are hidden by default and only exposed to other verified members of the same community when the player opts in. Gender is never displayed. Self-service account export and deletion endpoints ship in v1 (EU users). Open sign-up is guarded by email verification before visibility, per-IP sign-up rate limiting, and admin ban/remove. Payments data is limited to Stripe identifiers and amounts.

## 19. Decisions log

| Decision | Choice | Why |
|---|---|---|
| Backend | Single Rust binary (axum + sqlx) + Postgres | Minimal ops; comfortably handles expected load |
| Tenancy | Shared DB, `community_id` everywhere, extractor + RLS | Simplest with strong safety net |
| Identity | `users` (global) vs `players` (per community) | Multi-community and owner-as-player without migrations |
| Auth | Email OTP + optional password + Apple + Google; opaque sessions | Mobile-friendly, revocable, no vendor |
| Frontend | Expo for iOS/Android/web from day one | Mobile-first requirement; one theming pipeline |
| Distribution | One store listing per community, per-community Apple account | White-label requirement; Apple 4.3 |
| Doubles & mixed | In v1; entries carry `player_ids[]`; gender field for mixed | Product requirement; unified match model |
| Sign-up | Open, verification before visibility | Product requirement; abuse guardrails |
| Leagues | Round-robin boxes by UTR band; promotion/relegation | Standard club format; no mid-season jobs |
| Ranking | Ledger + rolling 52-week sum, per discipline | Auditable, decays naturally, rules replayable |
| Payments | Stripe Connect Express, hosted Checkout, per-player fees | Owner receives funds; no PCI scope |
| Platform fee | Per-community bps + fixed, changeable any time, recorded per charge | Product requirement for switching at will |
| Jobs | Postgres `jobs` table, in-process loop | No queue infrastructure |
| Realtime / chat | None in v1 | Proposal flow + polling suffices |
| Hosting, Phase 1 | One auto-stopping Fly Machine, Neon Free via the pooled URL, hourly scheduled `tick`, nightly `pg_dump` to R2 | About $0/month before revenue; no always-on compute; off-platform backups beyond Neon's 6 h PITR |
| Hosting, Phase 2 | Always-on Postgres on Fly, `min_machines_running = 1` | Payments need a warm server; no schema or code change |
| Connection pooling | Transaction-mode pooling for the app, direct URL for migrations and dumps | Server keeps only transaction-scoped state; proven by a pooled CI run |
| Image | Debian slim + `postgresql-client`, not distroless | Backups need `pg_dump` at least as new as the server |
| API client | `openapi-typescript` + `openapi-fetch`, generated and committed, drift-checked in CI | Smallest runtime; API changes show up in review diffs |

## 20. Open questions

Whether a community may run multiple concurrent leagues per discipline (the schema allows it; the UI may want to constrain it). How community onboarding (Stripe account, Apple developer account, DNS) is operated once there is more than one community. Whether friendly matches should ever count toward any ranking or an informal "activity" score. If the API ever runs on more than one Machine, moving the per-IP auth rate limits into Postgres.

Resolved since v0.1 (details in `docs/decisions.md`): mixed eligibility for `other` is a community setting (#65); a season does not finish over unresolved matches unless forced (#66–67); a league too small to play is cancelled at its start (#68); boxes stay auto-generated (#69); friendly mixed matches never check gender (#70); a score can be reported from `proposed` (#71); rate limits stay in memory for now (#72).

## 21. Suggested build order

1. Workspace, migrations, tenancy (extractor + RLS), auth, profiles, player directory.
2. Matches: proposals, scheduling, score reporting with format validation, confirm/dispute; friendly finder and match requests.
3. Leagues without fees (singles first, then doubles and mixed), standings, ledger and rankings, season jobs.
4. Payments: Stripe Connect onboarding, Checkout on registration, webhooks, refunds, platform fee controls.
5. Tournaments: draws, round deadlines, points.
6. Push notifications, deep links, notification preferences.
7. Per-community EAS build profile and first store submission.
