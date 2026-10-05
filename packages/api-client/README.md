# @racquetcollective/api-client

Typed client for the Racquet Collective REST API (`/api/v1`), generated from the server's OpenAPI
document. The Expo app (`apps/mobile`, spec §4) consumes it.

- `openapi.json` is the server's document, dumped by `racquetcollective-server openapi`.
- `src/schema.d.ts` is [`openapi-typescript`](https://openapi-ts.dev)'s types for it.
- `src/index.ts` is the only hand-written file: `createRacquetCollectiveClient` on top of
  [`openapi-fetch`](https://openapi-ts.dev/openapi-fetch/).

Both generated files are committed, so API changes show up in review. CI regenerates them and
fails if they differ from what is committed.

## Regenerating

After any change to a handler, its `#[utoipa::path]` or a type it exposes:

```sh
npm ci                              # at the repository root (npm workspaces), first time only
cd packages/api-client
npm run gen                         # cargo run -p racquetcollective-server -- openapi, then openapi-typescript
git add -A .
```

`npm run check` regenerates and fails on any diff (what CI runs); `npm run typecheck` runs
`tsc --noEmit`. `gen` needs a Rust toolchain but no database or server configuration.

## Using it

```ts
import { createRacquetCollectiveClient, isApiError } from "@racquetcollective/api-client";

const api = createRacquetCollectiveClient({
  baseUrl: "https://api.racquetcollective.app",
  community: "madrid-tc", // sent as X-RacquetCollective-Community; omit on a community host
  token: () => sessionStore.token, // Authorization: Bearer; a string works too
});

// In a browser on the community's own host: no community header (Host decides) and a
// cookie session instead of a token.
const web = createRacquetCollectiveClient({ baseUrl: location.origin, web: true });

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
