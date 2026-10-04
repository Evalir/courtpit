import type { ReactNode } from "react";
import { View } from "react-native";

import { createStyles } from "@/theme/ThemeProvider";
import { space } from "@/theme/tokens";
import { Card } from "@/ui/Card";
import { Icon, type IconName } from "@/ui/Icon";
import { Text } from "@/ui/Text";

/** A titled card of icon + label + value lines (profile, player page). */
export function DetailsCard({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Card>
      <Text variant="overline" tone="textMuted" accessibilityRole="header">
        {title}
      </Text>
      {children}
    </Card>
  );
}

/** One line of a details card; renders nothing without a value. */
export function Detail({
  icon,
  label,
  value,
}: {
  icon: IconName;
  label: string;
  value: string | null | undefined;
}) {
  const styles = useStyles();
  if (!value) return null;
  return (
    <View style={styles.line}>
      <Icon name={icon} tone="textMuted" />
      <View style={styles.text}>
        <Text variant="caption" tone="textMuted">
          {label}
        </Text>
        <Text>{value}</Text>
      </View>
    </View>
  );
}

const useStyles = createStyles(() => ({
  line: { flexDirection: "row", alignItems: "center", gap: space.md, paddingVertical: space.xs },
  text: { flex: 1 },
}));

/** "Head Speed MP", "RPM Blast · 24 kg": gear as one line. */
export function gearLine(racket?: string | null, strings?: string | null, tension?: number | null) {
  const stringing = [strings, tension ? `${tension} kg` : null].filter(Boolean).join(" · ");
  return { racket: racket ?? null, strings: stringing || null };
}

/** Social handles as "instagram: @ana" lines. */
export function socialsLine(socials: unknown): string | null {
  if (!socials || typeof socials !== "object") return null;
  const entries = Object.entries(socials as Record<string, unknown>).filter(
    (entry): entry is [string, string] => typeof entry[1] === "string" && entry[1] !== "",
  );
  return entries.length > 0
    ? entries.map(([network, handle]) => `${network}: ${handle}`).join("\n")
    : null;
}
