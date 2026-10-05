import type { components } from "@racquetcollective/api-client";
import { useState } from "react";
import { Pressable, View } from "react-native";

import { useApi } from "@/api/client";
import { formatUtr } from "@/features/format";
import { useDebounced } from "@/features/useDebounced";
import { createStyles } from "@/theme/ThemeProvider";
import { radius, space } from "@/theme/tokens";
import { Avatar } from "@/ui/Avatar";
import { Button } from "@/ui/Button";
import { Text } from "@/ui/Text";
import { TextField } from "@/ui/TextField";

type PlayerPublic = components["schemas"]["PlayerPublic"];

/**
 * Choose another member by name (partners, opponents). Searches the directory, which lists
 * verified, active members other than the viewer; `exclude` hides players already chosen.
 */
export function PlayerPicker({
  label,
  value,
  onChange,
  exclude = [],
}: {
  label: string;
  value: PlayerPublic | null;
  onChange: (player: PlayerPublic | null) => void;
  exclude?: readonly string[];
}) {
  const styles = useStyles();
  const { $api } = useApi();
  const [search, setSearch] = useState("");
  const q = useDebounced(search.trim());
  const results = $api.useQuery(
    "get",
    "/api/v1/players",
    { params: { query: { q, limit: 8 } } },
    { enabled: !value && q.length > 0 },
  );

  if (value) {
    return (
      <View style={styles.chosen}>
        <Avatar id={value.id} name={value.display_name} size={36} />
        <View style={styles.text}>
          <Text variant="caption" tone="textMuted">
            {label}
          </Text>
          <Text weight="semibold">{value.display_name}</Text>
        </View>
        <Button label="Change" variant="ghost" size="sm" onPress={() => onChange(null)} />
      </View>
    );
  }
  const options = (results.data?.items ?? []).filter((player) => !exclude.includes(player.id));
  return (
    <View style={{ gap: space.sm }}>
      <TextField
        label={label}
        value={search}
        onChangeText={setSearch}
        placeholder="Search by name"
        autoCorrect={false}
      />
      {options.map((player) => (
        <Pressable
          key={player.id}
          accessibilityRole="button"
          accessibilityLabel={`Choose ${player.display_name}`}
          onPress={() => {
            onChange(player);
            setSearch("");
          }}
          style={({ pressed }) => [styles.option, pressed && styles.pressed]}
        >
          <Avatar id={player.id} name={player.display_name} size={32} />
          <Text style={styles.text}>{player.display_name}</Text>
          <Text variant="caption" tone="textMuted">
            UTR {formatUtr(player.utr)}
          </Text>
        </Pressable>
      ))}
      {q && results.data && options.length === 0 ? (
        <Text variant="caption" tone="textMuted">
          Nobody called “{q}”.
        </Text>
      ) : null}
    </View>
  );
}

const useStyles = createStyles(({ colors }) => ({
  chosen: {
    flexDirection: "row",
    alignItems: "center",
    gap: space.md,
    padding: space.md,
    borderRadius: radius.md,
    borderWidth: 1,
    borderColor: colors.border,
    backgroundColor: colors.surface,
  },
  option: {
    flexDirection: "row",
    alignItems: "center",
    gap: space.md,
    paddingVertical: space.sm,
    paddingHorizontal: space.md,
    borderRadius: radius.md,
    backgroundColor: colors.surface,
  },
  pressed: { backgroundColor: colors.surfaceMuted },
  text: { flex: 1 },
}));
