import { QueryClientProvider, focusManager, type QueryClient } from "@tanstack/react-query";
import * as Linking from "expo-linking";
import { router, Stack, usePathname, type Href } from "expo-router";
import * as SplashScreen from "expo-splash-screen";
import { StatusBar } from "expo-status-bar";
import { useEffect, useRef, useState } from "react";
import { AppState, Platform } from "react-native";
import { SafeAreaProvider } from "react-native-safe-area-context";

import { ApiContext, createApi, type Api } from "@/api/client";
import { resolveApiConfig } from "@/api/config";
import { createQueryClient } from "@/api/queryClient";
import { NoAccess } from "@/session/NoAccess";
import { returnTo } from "@/session/returnTo";
import { SessionProvider, useSession, type Session } from "@/session/SessionProvider";
import { TenantProvider, useCommunity } from "@/tenant/TenantProvider";
import { fontFamilies } from "@/theme/fonts";
import { useTheme } from "@/theme/ThemeProvider";
import { ErrorState, LoadingState } from "@/ui/States";

void SplashScreen.preventAutoHideAsync();

/**
 * On web, a shared link straight to a detail screen still gets the tabs behind it (a back
 * button). Not on native: there, a signed-out deep link into a guarded screen with this anchor
 * redirects forever (`src/boot.test.tsx` covers it).
 */
export const unstable_settings = Platform.OS === "web" ? { anchor: "(tabs)" } : {};

type Setup = { api: Api; queryClient: QueryClient } | { error: unknown };

function setUp(): Setup {
  try {
    return { api: createApi(resolveApiConfig()), queryClient: createQueryClient() };
  } catch (error) {
    return { error };
  }
}

export default function RootLayout() {
  const [setup] = useState(setUp);
  useRefetchOnForeground();
  if ("error" in setup) return <MisconfiguredBuild error={setup.error} />;
  return (
    <SafeAreaProvider>
      <ApiContext value={setup.api}>
        <QueryClientProvider client={setup.queryClient}>
          <TenantProvider>
            <SessionProvider>
              <RootNavigator />
            </SessionProvider>
          </TenantProvider>
        </QueryClientProvider>
      </ApiContext>
    </SafeAreaProvider>
  );
}

/** Signed-in screens and sign-in screens, each reachable only in the matching state. */
function RootNavigator() {
  const { session } = useSession();
  const { colors, typography, dark } = useTheme();
  const community = useCommunity();
  useReturnAfterSignIn(session);

  if (session.status === "loading") return <LoadingState />;
  if (session.status === "error")
    return <ErrorState error={session.error} onRetry={session.retry} />;
  if (session.status === "no-access") return <NoAccess error={session.error} />;

  const signedIn = session.status === "signed-in";
  return (
    <>
      <StatusBar style={dark ? "light" : "dark"} />
      <Stack
        screenOptions={{
          title: community.name,
          headerStyle: { backgroundColor: colors.surface },
          headerTintColor: colors.primaryText,
          headerTitleStyle: {
            color: colors.text,
            fontFamily: fontFamilies[typography].semibold,
            fontWeight: "600",
          },
          headerShadowVisible: false,
          headerBackButtonDisplayMode: "minimal",
          contentStyle: { backgroundColor: colors.background },
        }}
      >
        <Stack.Protected guard={signedIn}>
          <Stack.Screen name="(tabs)" options={{ headerShown: false }} />
          <Stack.Screen name="matches/[id]/index" options={{ title: "Match" }} />
          <Stack.Screen
            name="matches/[id]/propose"
            options={{ title: "Propose a time", presentation: "modal" }}
          />
          <Stack.Screen
            name="matches/[id]/report"
            options={{ title: "Report the score", presentation: "modal" }}
          />
          <Stack.Screen name="players/[id]/index" options={{ title: "Player" }} />
          <Stack.Screen
            name="players/[id]/challenge"
            options={{ title: "Challenge", presentation: "modal" }}
          />
          <Stack.Screen
            name="requests/new"
            options={{ title: "New match request", presentation: "modal" }}
          />
          <Stack.Screen name="leagues/[id]" options={{ title: "League" }} />
          <Stack.Screen name="rankings/me" options={{ title: "Points history" }} />
          <Stack.Screen name="admin/index" options={{ title: "Club admin" }} />
          <Stack.Screen
            name="admin/leagues/new"
            options={{ title: "New league", presentation: "modal" }}
          />
          <Stack.Screen
            name="admin/leagues/[id]/edit"
            options={{ title: "Edit league", presentation: "modal" }}
          />
          <Stack.Screen
            name="admin/matches/[id]/resolve"
            options={{ title: "Settle the dispute", presentation: "modal" }}
          />
          <Stack.Screen
            name="profile/edit"
            options={{ title: "Edit profile", presentation: "modal" }}
          />
          <Stack.Screen
            name="profile/password"
            options={{ title: "Password", presentation: "modal" }}
          />
          <Stack.Screen
            name="profile/delete"
            options={{ title: "Delete account", presentation: "modal" }}
          />
        </Stack.Protected>
        <Stack.Protected guard={!signedIn}>
          <Stack.Screen name="sign-in" options={{ headerShown: false }} />
          <Stack.Screen name="verify" options={{ title: "" }} />
        </Stack.Protected>
      </Stack>
    </>
  );
}

/**
 * After signing in, carry on where the player was headed: the link that opened the app, a link
 * opened while signed out (native), or the screen they were on when the session expired.
 */
function useReturnAfterSignIn(session: Session) {
  const pathname = usePathname();
  const status = session.status;
  const expired = session.status === "signed-out" && session.expired;
  const lastPath = useRef<string | null>(null);
  const settled = useRef(false);
  const waiting = useRef(false);

  useEffect(() => {
    if (status === "signed-in") lastPath.current = pathname;
  }, [status, pathname]);

  useEffect(() => {
    if (status !== "signed-in" && status !== "signed-out") return;
    if (!settled.current) {
      // The opening link matters only if it hit the sign-in wall.
      settled.current = true;
      const initial = returnTo.initialUrl();
      if (status === "signed-out") void initial.then((url) => returnTo.remember(url));
    }
    if (status === "signed-out") {
      waiting.current = true;
      if (expired) returnTo.remember(lastPath.current);
    } else if (waiting.current) {
      waiting.current = false;
      const path = returnTo.take();
      // Pushed over Home, so back leads somewhere. A path from a link, checked by
      // `returnPathOf`; an unknown one lands on +not-found.
      if (path) router.push(path as Href);
    }
  }, [status, expired]);

  useEffect(() => {
    if (status !== "signed-out" || Platform.OS === "web") return;
    const subscription = Linking.addEventListener("url", ({ url }) => returnTo.remember(url));
    return () => subscription.remove();
  }, [status]);
}

/** A native build packaged without a community or API address (see `resolveApiConfig`). */
function MisconfiguredBuild({ error }: { error: unknown }) {
  useEffect(() => SplashScreen.hide(), []);
  return <ErrorState error={error} />;
}

/** On native, tell TanStack Query when the app returns to the foreground (web does this itself). */
function useRefetchOnForeground() {
  useEffect(() => {
    if (Platform.OS === "web") return;
    const subscription = AppState.addEventListener("change", (state) =>
      focusManager.setFocused(state === "active"),
    );
    return () => subscription.remove();
  }, []);
}
