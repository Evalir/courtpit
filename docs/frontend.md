# Courtpit — Frontend

**Status:** first slice (stack step F1 below), 2026-10-04. The architecture doc (§5, §6, §16) is
authoritative; this document records how the client realises it, and `docs/decisions.md` (#73
onwards) records the rulings it needed.

One Expo app in `apps/mobile` serves iOS, Android and the web from one codebase (spec §16). Each
community gets its own native build with its identity baked in; colors, typeface and feature
flags arrive at runtime from `GET /api/v1/tenant`, so a rebrand needs no store release.

## 1. Stack

| Concern | Choice |
|---|---|
| Framework | Expo SDK 57 (React Native 0.86, React 19.2), `react-native-web` for the web surface |
| Navigation | Expo Router: file-based routes in `src/app`, typed routes, `Stack.Protected` guards |
| Language | TypeScript, `strict` + `noUncheckedIndexedAccess` |
| API | `@courtpit/api-client` (`openapi-fetch`, types generated from the server's OpenAPI document) |
| Server state | TanStack Query 5, with `openapi-react-query` for typed `useQuery` / `useMutation` |
| Styling | React Native `StyleSheet` plus a runtime theme built from the community's branding; no UI kit |
| Icons, type | Ionicons (`@expo/vector-icons`); the system font or Inter, picked by `branding.typography` |
| Storage | `expo-secure-store` for the native session token; nothing on web (httpOnly cookie) |
| Tests | Jest (`jest-expo`) and React Native Testing Library 14 |
| Tooling | npm workspaces at the repository root, `eslint-config-expo`, Prettier (100 columns) |

There is deliberately no state library beyond TanStack Query (the server is the source of truth;
local state is component state) and no form library yet.

## 2. Layout

```
apps/mobile/
  app.config.ts            per-community identity from the build environment
  metro.config.js          dev proxy: /api → local courtpit-server
  .env.development         EXPO_PUBLIC_COMMUNITY=demo for `expo start`
  src/
    app/                   routes (every file is a screen; _layout files are navigators)
      _layout.tsx          providers, splash, signed-in / signed-out guards
      sign-in.tsx verify.tsx
      (tabs)/              Home, Play, Leagues, Rankings, Profile
      matches/[id].tsx leagues/[id].tsx players/[id].tsx
    api/                   config resolution, client + query hooks, errors, paging
    session/               token storage (native / web twins), SessionProvider, NoAccess
    tenant/                TenantProvider (branding → theme), CommunityMark
    theme/                 color math, palette derivation, tokens, fonts, createStyles
    ui/                    primitives: Text, Button, TextField, Card, Badge, Avatar, Screen, ...
    features/              domain helpers and composite components (matches, leagues, players)
```

Non-route code never lives under `src/app` (Expo Router would treat it as a screen).

## 3. Configuration and tenancy

The bundle reads two `EXPO_PUBLIC_*` variables (inlined at build time) in `src/api/config.ts`:

| Build | `EXPO_PUBLIC_COMMUNITY` | `EXPO_PUBLIC_API_URL` | Community resolved by | Session |
|---|---|---|---|---|
| Native, store build | baked per community (EAS profile) | `https://api.courtpit.app` | `X-Courtpit-Community` header | bearer token |
| Native, development | `demo` (`.env.development`) | empty → the dev server's host | header | bearer token |
| Web, production | unset | unset → the page's own origin | `Host` (`{slug}.courtpit.app` or custom domain) | httpOnly cookie |
| Web, development | `demo` | unset → `localhost:8081` | header (localhost is no community host) | httpOnly cookie |

The web app and the API share an origin in every environment. In production the API serves the
exported single-page app itself (step F6), so the browser talks to `{slug}.courtpit.app` only:
the session cookie is same-site, the host already names the community, and no CORS is needed. In
development `metro.config.js` proxies `/api/*`, `/healthz` and `/readyz` from the Expo dev server
to `COURTPIT_DEV_API` (default `http://127.0.0.1:8080`); Expo Go on a phone reaches the API the
same way, through the host it loaded the bundle from.

App identity for native builds (`COURTPIT_APP_NAME`, `COURTPIT_BUNDLE_ID`, `COURTPIT_SCHEME`) is
read by `app.config.ts`; per-community EAS build profiles that set these arrive with spec step 7.

## 4. Sign-in and sessions

- **Flow.** `sign-in` takes an email and calls `POST /auth/otp/request`; `verify` takes the
  6-digit code (submits itself at six digits, offers a resend) and calls `POST /auth/otp/verify`
  with a device label. Signing in through a community's app also joins it (decision 9), so there
  is no separate sign-up.
- **Storage.** Native keeps the bearer token in the keychain/keystore and in memory for the
  request middleware. Web sends `X-Courtpit-Client: web`, receives the httpOnly cookie and never
  sees the token (decision 10).
- **Boot.** The app asks `GET /auth/session` (native only when a token is stored). `401` →
  signed out; `403` → a "you can't open this community" screen offering *Join* (`POST /me/join`)
  and *Sign out*; a network failure keeps the stored session and offers a retry.
- **Expiry.** Any `unauthorized` answer, from any query or mutation, ends the session locally
  and drops every cached response except the community's.
- **Guards.** The root stack wraps signed-in and signed-out screens in `Stack.Protected`; a
  signed-out deep link lands on sign-in.
- **Not yet:** password sign-in and "set a password", Apple and Google, and returning to the
  deep link after signing in.

## 5. Theming (white-label)

Communities brand five colors in `branding.colors` — `primary`, `secondary`, `background`,
`surface`, `text` — any CSS hex or `rgb()` string. The client derives the rest of the palette
(`src/theme/theme.ts`) and guarantees legibility whatever was chosen:

- body text reaches 7:1 against the surface and secondary text 4.5:1 (WCAG AA), nudged towards
  black or white only as far as needed;
- `primaryText` (links, active tab) is the brand primary darkened or lightened to 4.5:1 on the
  background; the fill keeps the exact brand color, and text on it is white or black, whichever
  contrasts more;
- tints (`primarySoft`, the soft success/warning/danger fills) are mixed from the surface;
  success, warning and danger are fixed hues, never branded.

`branding.typography` picks a typeface the app ships: `inter` (four weights bundled) or anything
else for the system font. `display_name` and `logo_url` drive the sign-in hero (a tennis ball in
the accent color stands in for a missing or broken logo). `feature_flags.doubles` and
`mixed_doubles` hide the matching ranking tabs; `match_requests` will gate open match requests.

The UI is light-only for now (`userInterfaceStyle: "light"`): branding defines one palette, and
deriving a faithful dark variant of an arbitrary brand needs a design decision of its own.

Screens style themselves with `createStyles(theme => ({ ... }))`, which builds a `StyleSheet`
once per theme object; the theme object changes only when the tenant response does.

## 6. Information architecture

Five tabs, chosen around what a club player does weekly:

| Tab | Purpose |
|---|---|
| **Home** | "What needs me": scores to confirm or report, upcoming matches, matches to arrange, waiting on others, recent results |
| **Play** | Find a player (directory); open match requests and proposing a friendly join it next |
| **Leagues** | Seasons by status; a league's dates, format and box tables; registration next |
| **Rankings** | 52-week points per discipline (singles / doubles / mixed, by feature flag) |
| **Profile** | The player's profile, contact visibility and account; editing next |

Tab screens have no navigation header: a large title sits under the status bar. Detail screens
(match, league, player) are pushed on the root stack with a header and back button. On web a
link straight to a detail screen gets the tabs behind it (Expo Router `anchor`); on native the
anchor is off because it loops on signed-out deep links (decision 82).

Home sorts the player's matches (`features/matches/match.ts`, unit-tested):

| Section | Rule |
|---|---|
| Needs you | the other side reported a score (confirm or dispute), or a scheduled time has passed without a score (report) |
| Upcoming | scheduled, still ahead; soonest first |
| To arrange | `proposed`: no agreed time yet |
| Waiting on others | the viewer's side reported, or the result is disputed |
| Recent results | confirmed, resolved or walkover; newest five |

### Screen inventory

| Screen | Route | API | State |
|---|---|---|---|
| Sign in | `/sign-in` | `POST /auth/otp/request` | built |
| Verify code | `/verify?email=` | `POST /auth/otp/verify`, `/auth/otp/request` (resend) | built |
| Home | `/` | `GET /me`, `GET /matches` | built |
| Match | `/matches/[id]` | `GET /matches/{id}`, `GET /leagues/{id}`; `POST …/confirm`, `…/dispute`, `…/proposals/{id}/accept`, `…/decline` | built |
| Propose a time | `/matches/[id]/propose` (modal) | `POST /matches/{id}/proposals` | built |
| Report the score | `/matches/[id]/report` (modal) | `POST /matches/{id}/score` | built |
| Cancel a friendly | `/matches/[id]` | `POST /matches/{id}/cancel` | built |
| Play: directory | `/play` | `GET /players?q=` (cursor pages) | built |
| Player | `/players/[id]` | `GET /players/{id}` | built |
| Play: match requests, propose a friendly | `/play`, `/players/[id]` | `GET/POST /match-requests`, `…/join`, `…/leave`, `…/cancel`, `POST /matches` | F3 |
| Leagues | `/leagues` | `GET /leagues` | built |
| League | `/leagues/[id]` | `GET /leagues/{id}`, `GET /leagues/{id}/standings` | built |
| League: enter, partner invites, withdraw | `/leagues/[id]` | `GET/POST /leagues/{id}/entries`, `…/accept`, `…/decline`, `…/withdraw` | F4 |
| Rankings | `/rankings` | `GET /rankings?discipline=` | built |
| Points history | `/rankings/me` | `GET /rankings/events` | F5 |
| Profile | `/profile` | `GET /me` | built |
| Edit profile, password, export, delete account | `/profile/*` | `PATCH /me`, `PUT /auth/password`, `GET /me/export`, `DELETE /me` | F5 |
| Admin: leagues, disputes, moderation | `/admin/*` | `/admin/*` | F7 |

## 7. Data conventions

- **Queries** use `$api.useQuery("get", path, init)` from `useApi()`; keys are
  `["get", path, init]`, so invalidating by path works for every hook.
- **Freshness.** Data is fresh for 30 s and refetched when the app returns to the foreground
  (web: tab focus); pull-to-refresh everywhere. There is no realtime in v1 (spec §15).
- **Retries.** Network failures retry twice; API errors are answers and never retry.
- **Writes** go through `$api.useMutation`, then `refreshAfterWrite` invalidates everything but
  the community and the session (a confirmed league match moves standings and rankings too).
- **Lists** with cursors use `useCursorList` over the fetch client: `openapi-react-query`'s
  infinite query sends `cursor=0` for the first page, which the API rejects as a bad cursor.
- **Player names.** Matches, standings lines, entries and match requests carry `names` for the ids
  they list (decision 85); `nameLookup(view.names, me)` turns them into a lookup. The viewer
  reads "You"; an id a view did not name reads "Former member".
- **Errors** render the API's `message` (written for people, decision 2) via `describeError`;
  code switches on `error.code`.
- **Time.** ISO instants from the API are shown in the device's locale and time zone with
  `Intl.DateTimeFormat`. Pure helpers take "now" as an argument; screens use the time the data
  was fetched.

### Match actions

The match page's action card (`features/matches/MatchActions.tsx`, rules in `matchActions`)
offers, to players only: confirm or dispute a score the other side reported; accept or decline
the other side's open proposal (or wait on one's own); propose a time; report the score (from
`proposed` too, decision 71); cancel, for friendlies only (decision 19). The likelier next step
leads: proposing on a `proposed` match, reporting on a `scheduled` one.

- **Proposing** uses a day-and-slot picker (`WhenPicker`): the next four weeks as chips, then
  half-hour start times from 07:00 to 21:30, past slots disabled, in the device's time zone.
  The place defaults to where the match was last arranged; the player's preferred locations are
  one-tap suggestions.
- **Reporting** shows one row per set, the viewer's side in the first column: as many rows as
  it takes to win, then a deciding row once the sets are split, typed as the format says (set,
  pro set to 8, or match tiebreak in points). `checkScore` is a port of the domain crate's
  `validate_score`; both run `crates/domain/testdata/score_vectors.json`, so the form's verdict
  and the server's cannot drift apart (decision 86).

## 8. UI conventions

- Build screens from `src/ui` primitives; reach for raw `View`/`Text` styling only inside them.
- Text through `Text` with a `variant` (type scale in `theme/tokens.ts`) and a palette `tone`.
- Touch targets are at least 48 pt (buttons, inputs); interactive rows carry
  `accessibilityRole` and a spoken `accessibilityLabel`; headings use `accessibilityRole="header"`.
- On wide screens content sits in a centered 720 pt column.
- Copy uses typographic apostrophes and quotes (’ “ ”), en dashes in scores and ranges (6–4).

## 9. Testing

- Pure logic (color math, palette contrast across hostile brandings, score formatting, Home
  grouping, league dates and format descriptions) has plain unit tests next to the code.
- `src/boot.test.tsx` mounts the real route tree with a stubbed `fetch`: tenant → theme →
  signed-out deep link lands on sign-in; a failed boot offers a retry; a stored native token
  restores the session and Home shows the match that needs the player.
- CI (`mobile` job) runs Prettier, ESLint, `tsc` (after generating Expo Router's route types),
  Jest and a production web export.

## 10. Build order

| Step | Scope |
|---|---|
| F1 | This slice: workspace, theming, API layer, email-code sign-in, five tabs, read-only league/ranking/player screens, confirm/dispute and answering proposals |
| F2 | Match actions: propose a time (day-and-slot picker), format-aware score entry checked like the server, cancel a friendly (done) |
| F3 | Play: open match requests (list, create, join, leave), propose a friendly from a player page |
| F4 | League registration: enter, invite a partner, accept/decline, withdraw; "your box" view |
| F5 | Profile editing, password, data export, account deletion, points history; return to the deep link after sign-in |
| F6 | `courtpit-server` serves the web export (same origin, SPA fallback, a Node stage in the Dockerfile) |
| F7 | Admin: create/publish leagues, resolve disputes, walkovers, moderation |
| F8 | Native: Apple/Google sign-in, push and deep links (spec step 6), per-community EAS profiles and store submission (spec step 7) |

Payments (spec step 4) and tournaments (step 5) slot in after F4 as the server gains them.

## 11. Running it

```sh
npm ci                                   # repository root: installs every workspace
cargo run -p courtpit-server -- seed     # demo community `demo` (see README)
cargo run -p courtpit-server -- serve    # API on :8080
cd apps/mobile
npm run web                              # http://localhost:8081, /api proxied to :8080
npm start                                # Expo Go / simulators (same proxy)
```

Sign in as any seeded member (`lily.fernandez@example.com` has a score to confirm); with
`COURTPIT_MAILER=log` the code is in the server log. Checks: `npm run format:check`, `npm run
lint`, `npm run typecheck`, `npm test`, `npm run export:web`.
