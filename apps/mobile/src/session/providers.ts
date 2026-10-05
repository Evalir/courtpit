import { GoogleSignin, isSuccessResponse } from "@react-native-google-signin/google-signin";
import * as AppleAuthentication from "expo-apple-authentication";
import * as Crypto from "expo-crypto";
import { Platform } from "react-native";

/** An ID token for `POST /auth/oidc/{provider}`, with the nonce it was requested with. */
export interface ProviderToken {
  provider: "apple" | "google";
  id_token: string;
  nonce?: string;
}

/** Sign in with Apple: iOS 13+ only (spec §6 makes it mandatory beside Google there). */
export async function appleAvailable(): Promise<boolean> {
  return Platform.OS === "ios" && (await AppleAuthentication.isAvailableAsync());
}

const cancelled = (error: unknown) =>
  typeof error === "object" &&
  error !== null &&
  "code" in error &&
  (error.code === "ERR_REQUEST_CANCELED" || error.code === "SIGN_IN_CANCELLED");

/** Asks Apple for an ID token; null when the player backs out. */
export async function appleToken(): Promise<ProviderToken | null> {
  // Apple copies the nonce into the token as given; the server compares it as given.
  const nonce = Crypto.randomUUID();
  try {
    const credential = await AppleAuthentication.signInAsync({
      requestedScopes: [
        AppleAuthentication.AppleAuthenticationScope.FULL_NAME,
        AppleAuthentication.AppleAuthenticationScope.EMAIL,
      ],
      nonce,
    });
    return credential.identityToken
      ? { provider: "apple", id_token: credential.identityToken, nonce }
      : null;
  } catch (error) {
    if (cancelled(error)) return null;
    throw error;
  }
}

const googleWebClientId = process.env.EXPO_PUBLIC_GOOGLE_WEB_CLIENT_ID;
const googleIosClientId = process.env.EXPO_PUBLIC_GOOGLE_IOS_CLIENT_ID;

/**
 * Google sign-in needs the build's OAuth client ids: the web client id (the token's audience,
 * which the server lists in `RACQUETCOLLECTIVE_GOOGLE_CLIENT_IDS`) and, on iOS, the iOS client id.
 */
export function googleAvailable(): boolean {
  return Boolean(googleWebClientId) && (Platform.OS !== "ios" || Boolean(googleIosClientId));
}

let googleConfigured = false;

/** Asks Google for an ID token; null when the player backs out. */
export async function googleToken(): Promise<ProviderToken | null> {
  if (!googleConfigured) {
    GoogleSignin.configure({ webClientId: googleWebClientId, iosClientId: googleIosClientId });
    googleConfigured = true;
  }
  try {
    await GoogleSignin.hasPlayServices({ showPlayServicesUpdateDialog: true });
    const response = await GoogleSignin.signIn();
    if (!isSuccessResponse(response) || !response.data.idToken) return null;
    return { provider: "google", id_token: response.data.idToken };
  } catch (error) {
    if (cancelled(error)) return null;
    throw error;
  }
}
