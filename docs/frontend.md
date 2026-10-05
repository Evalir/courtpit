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
exported single-page app itself (`COURTPIT_WEB_DIR`, decision 88; see `docs/deploy.md`), so the
browser talks to `{slug}.courtpit.app` only:
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
- **Returning.** Signing in carries on where the player was headed, pushed over Home so back
  works: the link that opened the app (web reads the address before the router replaces it),
  a link opened while signed out (native), or the screen the session expired on
  (`session/returnTo.ts`, decision 92). Home, sign-in, verify and the delete screen are no
  destinations.
- **Passwords.** A signed-in player can set or change a password from Profile
  (`PUT /auth/password`); email codes keep working.
- **Not yet:** password sign-in on the sign-in screen, Apple and Google.

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
| **Home** | "What needs me": league invitations and entries that lost their partner, scores to confirm or report, upcoming matches, matches to arrange, waiting on others, recent results |
| **Play** | Open match requests (join one or post your own) and the player directory; challenge a player from their page |
| **Leagues** | Seasons by status, marked where the player is in or invited; a league's dates, format, registration and box tables (the player's box first) |
| **Rankings** | 52-week points per discipline (singles / doubles / mixed, by feature flag) |
| **Profile** | The player's profile and account: edit it, points history, password, data export, sign out, delete the account |

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
| Home | `/` | `GET /me`, `GET /matches`, `GET /me/entries`; `POST /leagues/{id}/entries/{entry_id}/accept`, `…/decline` | built |
| Match | `/matches/[id]` | `GET /matches/{id}`, `GET /leagues/{id}`; `POST …/confirm`, `…/dispute`, `…/proposals/{id}/accept`, `…/decline` | built |
| Propose a time | `/matches/[id]/propose` (modal) | `POST /matches/{id}/proposals` | built |
| Report the score | `/matches/[id]/report` (modal) | `POST /matches/{id}/score` | built |
| Cancel a friendly | `/matches/[id]` | `POST /matches/{id}/cancel` | built |
| Play: directory | `/play` | `GET /players?q=` (cursor pages) | built |
| Player | `/players/[id]` | `GET /players/{id}` | built |
| Play: open requests | `/play` | `GET /match-requests?fits_me=` (cursor pages), `POST /match-requests/{id}/join`, `…/leave`, `…/cancel` | built |
| New match request | `/requests/new` (modal) | `POST /match-requests`, `GET /players?q=` (partner) | built |
| Challenge a player | `/players/[id]/challenge` (modal) | `POST /matches` | built |
| Leagues | `/leagues` | `GET /leagues`, `GET /me/entries` | built |
| League | `/leagues/[id]` | `GET /leagues/{id}`, `GET /leagues/{id}/standings` | built |
| League: enter, partner invites, withdraw | `/leagues/[id]` | `GET/POST /leagues/{id}/entries`, `…/{entry_id}/accept`, `…/decline`, `…/partner`, `…/withdraw` | built |
| Rankings | `/rankings` | `GET /rankings?discipline=` | built |
| Points history | `/rankings/me` | `GET /rankings/events?player_id=` | built |
| Profile | `/profile` | `GET /me` | built |
| Edit profile | `/profile/edit` (modal) | `PATCH /me` | built |
| Password | `/profile/password` (modal) | `PUT /auth/password` | built |
| Download my data | `/profile` | `GET /me/export` (web: a JSON download; native: the share sheet) | built |
| Delete account | `/profile/delete` (modal) | `DELETE /me` | built |
| Club admin: banned members | `/admin` | `GET /players?status=banned`, `POST /admin/players/{id}/unban` | built |
| Player: ban or lift a ban (admins) | `/players/[id]` | `POST /admin/players/{id}/ban`, `…/unban` | built |
| Admin: leagues, disputes | `/admin/*` | `/admin/*` | F7 |

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
- **Cancelled** matches say who called them off (a player by name, anyone else as "a club
  admin") with the reason; a proposal a cancel or a report closed reads "Closed", not
  "replaced".

### League registration

A league taking entries shows a registration section (`features/leagues/EntryPanel.tsx`, rules in
`entries.ts`): invitations to the viewer (accept or decline), then either the way in or the
viewer's entry, with the number of complete entries beside the heading.

- **Entering.** Singles is one tap. Doubles and mixed take a partner from a directory search (they
  get an invitation; the entry completes when they accept) or "enter and look for a partner".
  Mixed explains up front when the profile's gender is undisclosed; anything else the community's
  rule decides, and the server's message is shown.
- **The viewer's entry** reads as entered (with whom), waiting for an invited partner, looking, or
  needing a partner (declined or lapsed). Until it is complete it can invite someone else or list
  itself as looking (`…/partner`, decision 90). Withdrawing asks first, naming the partner.
- **Looking for a partner** lists other solo entries; "Invite" either enters the viewer with that
  partner or points the viewer's solo entry at them. A player already invited reads "Invited".
- **Home** leads with a Leagues section while registration is open: invitations to answer inline,
  and entries whose partner fell through (to the league page). The summary line counts both.
- **Leagues** marks each league "You’re in", "Entry pending" or "Invited" from `GET /me/entries`.
- **An active league** lists the viewer's box first, titled "Your box".

### Profile and account

- **Edit profile** (`features/profile/profileForm.ts`, unit-tested) checks the server's limits
  as the player types and sends only what changed (`PATCH /me`); an emptied optional field
  clears it. UTR and tension accept a comma. Social handles are offered for Instagram, Facebook
  and X; other networks a profile holds are sent back untouched. Visibility uses a themed
  `Toggle` (React Native `Switch`).
- **Points history** groups the ledger by month, links league matches and season finishes,
  and greys out results older than 52 weeks (they no longer count, spec §12).
- **Download my data** saves `courtpit-export-<date>.json` on web and opens the share sheet
  on native (`saveExport.ts` / `.web.ts`, no new native module).
- **Delete account** lists what happens and asks for the account's email before
  `DELETE /me`, then signs the device out.
- Modals close with `closeModal(fallback)`: back when there is history, else the fallback
  (a modal opened straight from a link has nothing behind it).

### Club admin

Admins and owners get a "Club admin" button on Profile (`/admin`); everyone else never sees
admin screens, and the server refuses their requests anyway. `features/admin/roles.ts` mirrors
the server's moderation rule: never oneself or a deleted account, and only a lower rank (owners
may moderate admins).

- **Moderation.** A member's profile shows a "Banned" badge and, to admins who outrank them,
  "Ban from the club" (asks first) or "Lift the ban". The hub lists banned members
  (`GET /players?status=banned`, admins only, decision 93) with "Lift the ban" on each.

### Match requests

Play opens on the community's open requests when the `match_requests` feature is on (the
directory sits beside them), with a "Fits my level" filter (`fits_me`). A card shows the
window, place, level band, who is in and the spots left; the viewer can join (greyed out
outside the band, decision 27), leave, or cancel their own. Whoever fills the last spot lands on
the new match. Posting a request (`/requests/new`) takes the discipline (as the community's
features allow), a partner for doubles (optional), a start from the same `WhenPicker` and a
length (1, 1½, 2 or 3 hours), a level (any, or ±1.00 around the poster's UTR) and a place.
A player page offers "Challenge to a match": a singles friendly, optionally with a first
proposed time and place, that opens on its match page (decision 89).

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
  restores the session and Home shows the match and the league invitation that need the player. Profile, ledger and return-to rules have unit tests next to them.
- CI (`mobile` job) runs Prettier, ESLint, `tsc` (after generating Expo Router's route types),
  Jest and a production web export.

## 10. Build order

| Step | Scope |
|---|---|
| F1 | This slice: workspace, theming, API layer, email-code sign-in, five tabs, read-only league/ranking/player screens, confirm/dispute and answering proposals |
| F2 | Match actions: propose a time (day-and-slot picker), format-aware score entry checked like the server, cancel a friendly (done) |
| F3 | Play: open match requests (list, create, join, leave), propose a friendly from a player page (done) |
| F4 | League registration: enter, invite a partner, accept/decline, withdraw; "your box" view (done) |
| F5 | Profile editing, password, data export, account deletion, points history; return to the deep link after sign-in (done) |
| F6 | `courtpit-server` serves the web export (same origin, SPA fallback, a Node stage in the Dockerfile) (done) |
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
