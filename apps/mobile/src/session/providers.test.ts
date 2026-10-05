import { GoogleSignin } from "@react-native-google-signin/google-signin";
import * as AppleAuthentication from "expo-apple-authentication";

jest.mock("expo-crypto", () => ({ randomUUID: () => "2f1e7c56-8a3b-4c2d-9e10-5b6a7c8d9e0f" }));

type Providers = typeof import("./providers");

/** Loads the module with the given Google client ids (read at load time). */
function load(env: Record<string, string | undefined>): Providers {
  const saved = { ...process.env };
  Object.assign(process.env, env);
  let providers: Providers | undefined;
  jest.isolateModules(() => {
    providers = jest.requireActual<Providers>("./providers");
  });
  process.env = saved;
  if (!providers) throw new Error("providers did not load");
  return providers;
}

describe("Apple", () => {
  it("sends the nonce it asked Apple with, and backs out quietly", async () => {
    const { appleToken } = load({});
    jest
      .mocked(AppleAuthentication.signInAsync)
      .mockImplementationOnce(
        async (options) => ({ identityToken: `jwt-${options?.nonce}` }) as never,
      );
    const token = await appleToken();
    expect(token?.provider).toBe("apple");
    expect(token?.id_token).toBe(`jwt-${token?.nonce}`);
    expect(token?.nonce).toMatch(/^[0-9a-f-]{36}$/);

    jest
      .mocked(AppleAuthentication.signInAsync)
      .mockRejectedValueOnce(
        Object.assign(new Error("cancelled"), { code: "ERR_REQUEST_CANCELED" }),
      );
    await expect(appleToken()).resolves.toBeNull();
  });
});

describe("Google", () => {
  it("needs the client ids the build was given", () => {
    // Jest runs as iOS, which also needs the iOS client id.
    expect(load({}).googleAvailable()).toBe(false);
    expect(load({ EXPO_PUBLIC_GOOGLE_WEB_CLIENT_ID: "web" }).googleAvailable()).toBe(false);
    expect(
      load({
        EXPO_PUBLIC_GOOGLE_WEB_CLIENT_ID: "web",
        EXPO_PUBLIC_GOOGLE_IOS_CLIENT_ID: "ios",
      }).googleAvailable(),
    ).toBe(true);
  });

  it("returns Google's ID token, or nothing when the player backs out", async () => {
    const { googleToken } = load({ EXPO_PUBLIC_GOOGLE_WEB_CLIENT_ID: "web" });
    jest
      .mocked(GoogleSignin.signIn)
      .mockResolvedValueOnce({ type: "success", data: { idToken: "google-jwt" } } as never);
    await expect(googleToken()).resolves.toEqual({ provider: "google", id_token: "google-jwt" });
    expect(GoogleSignin.configure).toHaveBeenCalledWith({
      webClientId: "web",
      iosClientId: undefined,
    });
    jest
      .mocked(GoogleSignin.signIn)
      .mockResolvedValueOnce({ type: "cancelled", data: null } as never);
    await expect(googleToken()).resolves.toBeNull();
  });
});
