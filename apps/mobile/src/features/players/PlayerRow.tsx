import type { components } from "@racquetcollective/api-client";
import { router } from "expo-router";
import { Pressable, View } from "react-native";

import { formatUtr, playPrefLabel } from "@/features/format";
import { createStyles } from "@/theme/ThemeProvider";
import { radius, space } from "@/theme/tokens";
import { Avatar } from "@/ui/Avatar";
import { Icon } from "@/ui/Icon";
import { Text } from "@/ui/Text";

type PlayerPublic = components["schemas"]["PlayerPublic"];

/** A directory line: initials, name, level and where they like to play. */
export function PlayerRow({ player }: { player: PlayerPublic }) {
  const styles = useStyles();
  const details = [`UTR ${formatUtr(player.utr)}`, playPrefLabel[player.play_pref]].join(" · ");
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={`${player.display_name}, ${details}`}
      onPress={() => router.push({ pathname: "/players/[id]", params: { id: player.id } })}
      style={({ pressed }) => [styles.row, pressed && styles.pressed]}
    >
      <Avatar id={player.id} name={player.display_name} />
      <View style={styles.text}>
        <Text variant="subheading" numberOfLines={1}>
          {player.display_name}
        </Text>
        <Text variant="label" tone="textMuted" numberOfLines={1}>
          {details}
        </Text>
        {player.preferred_locations.length > 0 ? (
          <Text variant="caption" tone="textMuted" numberOfLines={1}>
            {player.preferred_locations.join(", ")}
          </Text>
        ) : null}
      </View>
      <Icon name="chevron-forward" tone="textMuted" />
    </Pressable>
  );
}

const useStyles = createStyles(({ colors }) => ({
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: space.md,
    padding: space.md,
    backgroundColor: colors.surface,
    borderRadius: radius.lg,
    borderWidth: 1,
    borderColor: colors.border,
  },
  pressed: { backgroundColor: colors.surfaceMuted },
  text: { flex: 1, gap: space.xxs },
}));
