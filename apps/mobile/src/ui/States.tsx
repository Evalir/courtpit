import type { ReactNode } from "react";
import { ActivityIndicator, View } from "react-native";

import { describeError } from "@/api/errors";
import { createStyles, useTheme } from "@/theme/ThemeProvider";
import { space } from "@/theme/tokens";

import { Button } from "./Button";
import { Icon, type IconName } from "./Icon";
import { Text } from "./Text";

/** Fills the screen with a spinner. */
export function LoadingState() {
  const styles = useStyles();
  const { colors } = useTheme();
  return (
    <View style={styles.fill}>
      <ActivityIndicator color={colors.primary} size="large" />
    </View>
  );
}

/** Explains a failed load and offers a retry. */
export function ErrorState({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  return (
    <EmptyState icon="cloud-offline-outline" title="Couldn’t load this" body={describeError(error)}>
      {onRetry ? <Button label="Try again" variant="secondary" onPress={onRetry} /> : null}
    </EmptyState>
  );
}

/** A friendly placeholder for an empty list or a failed load. */
export function EmptyState({
  icon,
  title,
  body,
  children,
}: {
  icon: IconName;
  title: string;
  body?: string;
  children?: ReactNode;
}) {
  const styles = useStyles();
  return (
    <View style={styles.fill}>
      <Icon name={icon} size={40} tone="textMuted" />
      <Text variant="subheading" align="center">
        {title}
      </Text>
      {body ? (
        <Text tone="textMuted" align="center" style={styles.body}>
          {body}
        </Text>
      ) : null}
      {children}
    </View>
  );
}

const useStyles = createStyles(({ colors }) => ({
  fill: {
    flexGrow: 1,
    alignItems: "center",
    justifyContent: "center",
    gap: space.sm,
    padding: space.xxl,
    backgroundColor: colors.background,
  },
  body: { maxWidth: 360 },
}));
