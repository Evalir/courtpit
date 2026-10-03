# @courtpit/api-client

Typed client for the Courtpit REST API (`/api/v1`), generated from the server's OpenAPI
document. The Expo app (`apps/mobile`, spec §4) consumes it.

- `openapi.json` is the server's document, dumped by `courtpit-server openapi`.
- `src/schema.d.ts` is [`openapi-typescript`](https://openapi-ts.dev)'s types for it.
- `src/index.ts` is the only hand-written file: `createCourtpitClient` on top of
  [`openapi-fetch`](https://openapi-ts.dev/openapi-fetch/).

Both generated files are committed, so API changes show up in review. CI regenerates them and
fails if they differ from what is committed.

## Regenerating

After any change to a handler, its `#[utoipa::path]` or a type it exposes:

```sh
cd packages/api-client
npm ci          # first time only
npm run gen     # cargo run -p courtpit-server -- openapi, then openapi-typescript
git add -A .
```

`npm run check` regenerates and fails on any diff (what CI runs); `npm run typecheck` runs
`tsc --noEmit`. `gen` needs a Rust toolchain but no database or server configuration.

## Using it

```ts
import { createCourtpitClient, isApiError } from "@courtpit/api-client";

const api = createCourtpitClient({
  baseUrl: "https://api.courtpit.app",
  community: "madrid-tc", // sent as X-Courtpit-Community
  token: () => sessionStore.token, // Authorization: Bearer; a string works too
});

const { data, error } = await api.GET("/api/v1/matches/{id}", {
  params: { path: { id } },
});
if (error) {
  // Every error body is { error: { code, message } }; switch on `code`.
  if (isApiError(error) && error.error.code === "not_found") { /* ... */ }
} else {
  data.status; // typed from the spec
}
```

Paths, parameters, bodies and responses are all checked against the spec at compile time.
The generated `paths`, `components` and `operations` types are re-exported for the app's own
type annotations (`components["schemas"]["MatchView"]`).
