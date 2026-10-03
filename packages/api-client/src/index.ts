/**
 * Typed client for the Courtpit REST API (`/api/v1`), built on `openapi-fetch`.
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

export interface CourtpitClientOptions {
  /** Server origin, e.g. `https://api.courtpit.app` (no trailing path). */
  baseUrl: string;
  /** Community slug, sent as `X-Courtpit-Community` on every request. */
  community: string;
  /**
   * Session token, sent as `Authorization: Bearer <token>`. Pass a function to read it per
   * request (e.g. from secure storage), so the client can be created before sign-in.
   */
  token?: string | (() => string | undefined);
}

/** Creates a client whose requests carry the community header and, when set, the session. */
export function createCourtpitClient({ baseUrl, community, token }: CourtpitClientOptions) {
  const client = createClient<paths>({ baseUrl });
  const headers: Middleware = {
    onRequest({ request }) {
      request.headers.set("X-Courtpit-Community", community);
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

/** The client returned by {@link createCourtpitClient}. */
export type CourtpitClient = ReturnType<typeof createCourtpitClient>;
