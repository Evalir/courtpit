import { isApiError } from "@racquetcollective/api-client";

/**
 * Errors reach the UI in two shapes: the API's `{ error: { code, message } }` body (thrown by
 * `openapi-react-query` for non-2xx responses) or an exception from `fetch` itself.
 */

/** The API's stable error code (`not_found`, `unauthorized`, ...), if this is an API error. */
export function errorCode(error: unknown): string | undefined {
  return isApiError(error) ? error.error.code : undefined;
}

/** A sentence to show the user. API messages are written for people and never leak internals. */
export function describeError(error: unknown): string {
  if (isApiError(error)) return error.error.message;
  if (error instanceof TypeError) {
    return "Can’t reach the server. Check your connection and try again.";
  }
  return "Something went wrong. Please try again.";
}
