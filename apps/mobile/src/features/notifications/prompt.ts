/** How long "Not now" keeps the push prompt away. */
export const PROMPT_SNOOZE_DAYS = 30;

/** Whether to offer push again, given when the player last said "Not now" (ISO), if ever. */
export function promptDue(dismissedAt: string | null, now: Date): boolean {
  if (!dismissedAt) return true;
  const at = Date.parse(dismissedAt);
  return Number.isNaN(at) || now.getTime() - at >= PROMPT_SNOOZE_DAYS * 86_400_000;
}
