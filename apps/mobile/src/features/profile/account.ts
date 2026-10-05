/** The server's password length limits (`PUT /auth/password`). */
export const PASSWORD_MIN = 10;
export const PASSWORD_MAX = 256;

/** What is wrong with a new password and its repetition, if anything. */
export function passwordProblem(
  password: string,
  again: string,
): { password?: string; again?: string } | null {
  const length = [...password].length;
  if (length < PASSWORD_MIN) return { password: `At least ${PASSWORD_MIN} characters.` };
  if (length > PASSWORD_MAX) return { password: `At most ${PASSWORD_MAX} characters.` };
  if (again !== password) return { again: "The two passwords differ." };
  return null;
}

/** Deleting asks the player to type their email: a slip of the thumb can't do it. */
export function deletionConfirmed(typed: string, email: string): boolean {
  return typed.trim().toLowerCase() === email.trim().toLowerCase();
}

/** `courtpit-export-2026-10-04.json`. */
export function exportFileName(now: Date): string {
  return `courtpit-export-${now.toISOString().slice(0, 10)}.json`;
}
