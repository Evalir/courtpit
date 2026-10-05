import type { CourtpitClient } from "@courtpit/api-client";

/** Whether this device can show pushes, and whether the player let it. */
export type PushPermission = "granted" | "undetermined" | "denied" | "unavailable";

// The web app has no push (decision 97): scores to confirm arrive by email instead.

export async function pushPermission(): Promise<PushPermission> {
  return "unavailable";
}

export async function registerForPush(
  _fetch: CourtpitClient,
  _ask: boolean,
): Promise<PushPermission> {
  return "unavailable";
}

export async function forgetPushDevice(_fetch: CourtpitClient): Promise<void> {}

export function useNotificationTaps(_signedIn: boolean) {}
