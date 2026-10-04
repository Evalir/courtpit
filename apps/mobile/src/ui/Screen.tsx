import type { ReactNode } from "react";
import { RefreshControl, ScrollView, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { createStyles, useTheme } from "@/theme/ThemeProvider";
import { maxContentWidth, space } from "@/theme/tokens";

import { Text } from "./Text";

/**
 * A scrolling screen body on the community background, centered in a readable column on wide
 * screens. Tab screens pass `title` (they have no navigation header, so the title sits below
 * the status bar); stack screens leave it to their header. `onRefresh` adds pull-to-refresh.
 */
export function Screen({
  title,
  eyebrow,
  children,
  refreshing = false,
  onRefresh,
}: {
  title?: string;
  eyebrow?: string;
  children: ReactNode;
  refreshing?: boolean;
  onRefresh?: () => void;
}) {
  const styles = useScreenStyles();
  const { colors } = useTheme();
  const top = useTitleInset(!!title);
  return (
    <ScrollView
      style={styles.screen}
      contentContainerStyle={[styles.content, top]}
      keyboardShouldPersistTaps="handled"
      refreshControl={
        onRefresh ? (
          <RefreshControl
            refreshing={refreshing}
            onRefresh={onRefresh}
            tintColor={colors.primary}
          />
        ) : undefined
      }
    >
      {title ? <ScreenTitle title={title} eyebrow={eyebrow} /> : null}
      {children}
    </ScrollView>
  );
}

/** Top padding for a screen that draws its own title under the status bar. */
export function useTitleInset(enabled = true) {
  const insets = useSafeAreaInsets();
  return enabled ? { paddingTop: insets.top + space.xl } : null;
}

/** A tab screen's large title, with an optional small line above it. */
export function ScreenTitle({ title, eyebrow }: { title: string; eyebrow?: string }) {
  return (
    <View style={{ gap: space.xxs }}>
      {eyebrow ? (
        <Text variant="overline" tone="primaryText">
          {eyebrow}
        </Text>
      ) : null}
      <Text variant="title" accessibilityRole="header">
        {title}
      </Text>
    </View>
  );
}

/** Groups content under a heading inside a screen. */
export function Section({
  title,
  aside,
  children,
}: {
  title: string;
  aside?: ReactNode;
  children: ReactNode;
}) {
  const styles = useScreenStyles();
  return (
    <View style={styles.section}>
      <View style={styles.sectionHeader}>
        <Text variant="overline" tone="textMuted" accessibilityRole="header">
          {title}
        </Text>
        {aside}
      </View>
      {children}
    </View>
  );
}

/** Styles for screens that scroll with a `FlatList` instead of `Screen`. */
export const useScreenStyles = createStyles(({ colors }) => ({
  screen: { flex: 1, backgroundColor: colors.background },
  content: {
    width: "100%",
    maxWidth: maxContentWidth,
    alignSelf: "center",
    padding: space.lg,
    paddingBottom: space.xxxl,
    gap: space.xl,
  },
  list: {
    width: "100%",
    maxWidth: maxContentWidth,
    alignSelf: "center",
    padding: space.lg,
    paddingBottom: space.xxxl,
  },
  separator: { height: space.sm },
  section: { gap: space.sm },
  sectionHeader: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    paddingHorizontal: space.xs,
  },
}));
