import type { components } from "@courtpit/api-client";
import * as SplashScreen from "expo-splash-screen";
import { createContext, use, useEffect, useMemo, useState, type ReactNode } from "react";

import { useApi } from "@/api/client";
import { loadTypography } from "@/theme/fonts";
import { ThemeProvider } from "@/theme/ThemeProvider";
import { themeFromBranding, type Theme } from "@/theme/theme";
import { ErrorState, LoadingState } from "@/ui/States";

type TenantInfo = components["schemas"]["TenantInfo"];

/** The community this app instance belongs to, ready to render. */
export interface Community {
  slug: string;
  /** `branding.display_name`, else the community's name. */
  name: string;
  logoUrl: string | null;
  /** Feature flags; a flag the community hasn't set is on. */
  features: { doubles: boolean; mixed: boolean; matchRequests: boolean };
  theme: Theme;
}

/** Turns `GET /api/v1/tenant` into what the UI needs. */
export function communityFrom(tenant: TenantInfo): Community {
  const flags = tenant.branding.feature_flags ?? {};
  return {
    slug: tenant.slug,
    name: tenant.branding.display_name || tenant.name,
    logoUrl: tenant.branding.logo_url ?? null,
    features: {
      doubles: flags.doubles ?? true,
      mixed: flags.mixed_doubles ?? true,
      matchRequests: flags.match_requests ?? true,
    },
    theme: themeFromBranding(tenant.branding),
  };
}

const CommunityContext = createContext<Community | null>(null);

/**
 * Loads the community at boot and themes everything below it (spec §5: the client fetches
 * `GET /api/v1/tenant` and applies the theme at runtime). Children render only once the
 * branding and its typeface are ready, so nothing flashes in the wrong colors.
 */
export function TenantProvider({ children }: { children: ReactNode }) {
  const { $api } = useApi();
  const tenant = $api.useQuery("get", "/api/v1/tenant", undefined, { staleTime: 5 * 60_000 });
  // One object per response: themed styles are cached per theme object (`createStyles`).
  const community = useMemo(() => (tenant.data ? communityFrom(tenant.data) : null), [tenant.data]);
  const typography = community?.theme.typography;
  const [fontsReady, setFontsReady] = useState<string | null>(null);

  useEffect(() => {
    if (!typography) return;
    // A font that fails to load falls back to the system face rather than blocking the app.
    void loadTypography(typography)
      .catch((error: unknown) => console.warn("font load failed", error))
      .finally(() => setFontsReady(typography));
  }, [typography]);

  const ready = !!community && fontsReady === typography;
  useEffect(() => {
    // The native splash screen covers boot until the app can draw in the community's colors.
    if (ready || tenant.error) SplashScreen.hide();
  }, [ready, tenant.error]);

  if (tenant.error)
    return <ErrorState error={tenant.error} onRetry={() => void tenant.refetch()} />;
  if (!ready) return <LoadingState />;
  return (
    <CommunityContext value={community}>
      <ThemeProvider theme={community.theme}>{children}</ThemeProvider>
    </CommunityContext>
  );
}

/** The current community. */
export function useCommunity(): Community {
  const community = use(CommunityContext);
  if (!community) throw new Error("useCommunity outside TenantProvider");
  return community;
}
