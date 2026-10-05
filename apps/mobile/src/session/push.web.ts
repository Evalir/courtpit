import type { RacquetCollectiveClient } from "@racquetcollective/api-client";

/** Whether this device can show pushes, and whether the player let it. */
export type PushPermission = "granted" | "undetermined" | "denied" | "unavailable";

// The web app has no push (decision 97): scores to confirm arrive by email instead.

export async function pushPermission(): Promise<PushPermission> {
  return "unavailable";
}

export async function registerForPush(
  _fetch: RacquetCollectiveClient,
  _ask: boolean,
): Promise<PushPermission> {
  return "unavailable";
}

export async function forgetPushDevice(_fetch: RacquetCollectiveClient): Promise<void> {}

export function useNotificationTaps(_signedIn: boolean) {}
