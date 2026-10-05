/** An ID token for `POST /auth/oidc/{provider}`, with the nonce it was requested with. */
export interface ProviderToken {
  provider: "apple" | "google";
  id_token: string;
  nonce?: string;
}

// The web app signs in by email code or password (decision 100).

export async function appleAvailable(): Promise<boolean> {
  return false;
}

export async function appleToken(): Promise<ProviderToken | null> {
  return null;
}

export function googleAvailable(): boolean {
  return false;
}

export async function googleToken(): Promise<ProviderToken | null> {
  return null;
}
