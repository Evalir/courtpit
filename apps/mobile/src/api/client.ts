import {
  createRacquetCollectiveClient,
  type RacquetCollectiveClient,
  type paths,
} from "@racquetcollective/api-client";
import createQueryHooks, { type OpenapiQueryClient } from "openapi-react-query";
import { createContext, use } from "react";

import { sessionToken } from "@/session/token";

import type { ApiConfig } from "./config";

/** The typed transport plus the TanStack Query hooks built on it. */
export interface Api {
  /** `openapi-fetch` client for imperative calls (sign-in, sign-out). */
  fetch: RacquetCollectiveClient;
  /** `$api.useQuery("get", "/api/v1/...")` / `$api.useMutation(...)`, typed from the spec. */
  $api: OpenapiQueryClient<paths>;
  config: ApiConfig;
}

/** Builds the API for a configuration; the session token is read per request. */
export function createApi(config: ApiConfig): Api {
  const fetch = createRacquetCollectiveClient({ ...config, token: sessionToken.current });
  return { fetch, $api: createQueryHooks(fetch), config };
}

export const ApiContext = createContext<Api | null>(null);

/** The app's API client and query hooks. */
export function useApi(): Api {
  const api = use(ApiContext);
  if (!api) throw new Error("useApi outside ApiContext");
  return api;
}
