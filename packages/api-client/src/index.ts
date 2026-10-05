/**
 * Typed client for the Racquet Collective REST API (`/api/v1`), built on `openapi-fetch`.
 *
 * `schema.d.ts` is generated from the server's OpenAPI document (`npm run gen`); this file is
 * the only hand-written code in the package.
 */
import createClient, { type Middleware } from "openapi-fetch";

import type { components, paths } from "./schema";

export type { components, operations, paths } from "./schema";

/** The JSON body of every error response: `{ error: { code, message } }`. Switch on `code`. */
export type ApiErrorBody = components["schemas"]["ErrorBody"];

/** Narrows the `error` of an `openapi-fetch` result to the API's error shape. */
export function isApiError(error: unknown): error is ApiErrorBody {
  const detail = (error as Partial<ApiErrorBody> | null | undefined)?.error;
  return typeof detail?.code === "string" && typeof detail.message === "string";
}

export interface RacquetCollectiveClientOptions {
  /** Server origin, e.g. `https://api.racquetcollective.app` (no trailing path). */
  baseUrl: string;
  /**
   * Community slug, sent as `X-RacquetCollective-Community` on every request. Leave it out on a
   * community's own host (`{slug}.racquetcollective.app` or its custom domain): the server then resolves
   * the community from `Host`.
   */
  community?: string;
  /**
   * Browser client: sends `X-RacquetCollective-Client: web`, so sign-in sets the httpOnly session cookie
   * instead of returning a token (the browser then sends the cookie by itself).
   */
  web?: boolean;
  /**
   * Session token, sent as `Authorization: Bearer <token>`. Pass a function to read it per
   * request (e.g. from secure storage), so the client can be created before sign-in.
   */
  token?: string | (() => string | undefined);
}

/** Creates a client whose requests carry the community header and, when set, the session. */
export function createRacquetCollectiveClient({
  baseUrl,
  community,
  web,
  token,
}: RacquetCollectiveClientOptions) {
  const client = createClient<paths>({ baseUrl });
  const headers: Middleware = {
    onRequest({ request }) {
      if (community) {
        request.headers.set("X-RacquetCollective-Community", community);
      }
      if (web) {
        request.headers.set("X-RacquetCollective-Client", "web");
      }
      const session = typeof token === "function" ? token() : token;
      if (session) {
        request.headers.set("Authorization", `Bearer ${session}`);
      }
      return request;
    },
  };
  client.use(headers);
  return client;
}

/** The client returned by {@link createRacquetCollectiveClient}. */
export type RacquetCollectiveClient = ReturnType<typeof createRacquetCollectiveClient>;
