import * as SecureStore from "expo-secure-store";

const KEY = "courtpit.session";
let current: string | undefined;

/**
 * Native session storage: the bearer token lives in the keychain / keystore and in memory for
 * the request middleware, which needs it synchronously. `token.web.ts` is the browser twin.
 */
export const sessionToken = {
  /** Whether the app holds the session itself (native) rather than the browser (web cookie). */
  stored: true,
  current: (): string | undefined => current,
  async load(): Promise<string | undefined> {
    current = (await SecureStore.getItemAsync(KEY)) ?? undefined;
    return current;
  },
  async save(token: string): Promise<void> {
    await SecureStore.setItemAsync(KEY, token);
    current = token;
  },
  async clear(): Promise<void> {
    current = undefined;
    await SecureStore.deleteItemAsync(KEY);
  },
};
