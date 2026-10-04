import { useState } from "react";
import { Image } from "react-native";

import { useTheme } from "@/theme/ThemeProvider";
import { TennisBall } from "@/ui/TennisBall";

import { useCommunity } from "./TenantProvider";

/** The community's logo (`branding.logo_url`), or a ball in its accent color without one. */
export function CommunityMark({ size = 64 }: { size?: number }) {
  const { name, logoUrl } = useCommunity();
  const { colors } = useTheme();
  const [failed, setFailed] = useState(false);
  if (logoUrl && !failed) {
    return (
      <Image
        source={{ uri: logoUrl }}
        accessibilityLabel={`${name} logo`}
        onError={() => setFailed(true)}
        style={{ width: size, height: size, borderRadius: size / 4 }}
        resizeMode="contain"
      />
    );
  }
  return <TennisBall size={size} color={colors.accent} />;
}
