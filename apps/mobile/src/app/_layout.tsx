import { QueryClientProvider, focusManager, type QueryClient } from "@tanstack/react-query";
import { Stack } from "expo-router";
import * as SplashScreen from "expo-splash-screen";
import { StatusBar } from "expo-status-bar";
import { useEffect, useState } from "react";
import { AppState, Platform } from "react-native";
import { SafeAreaProvider } from "react-native-safe-area-context";

import { ApiContext, createApi, type Api } from "@/api/client";
import { resolveApiConfig } from "@/api/config";
import { createQueryClient } from "@/api/queryClient";
import { NoAccess } from "@/session/NoAccess";
import { SessionProvider, useSession } from "@/session/SessionProvider";
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
        </Stack.Protected>
        <Stack.Protected guard={!signedIn}>
          <Stack.Screen name="sign-in" options={{ headerShown: false }} />
          <Stack.Screen name="verify" options={{ title: "" }} />
        </Stack.Protected>
      </Stack>
    </>
  );
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
