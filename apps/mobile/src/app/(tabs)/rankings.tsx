import type { components } from "@courtpit/api-client";
import { router } from "expo-router";
import { useState } from "react";
import { Pressable, View } from "react-native";

import { useApi } from "@/api/client";
import { formatDateTime } from "@/features/format";
import { useSignedIn } from "@/session/SessionProvider";
import { useCommunity } from "@/tenant/TenantProvider";
import { createStyles } from "@/theme/ThemeProvider";
import { radius, space } from "@/theme/tokens";
import { Avatar } from "@/ui/Avatar";
import { Button } from "@/ui/Button";
import { Screen } from "@/ui/Screen";
import { Segmented } from "@/ui/Segmented";
import { EmptyState, ErrorState, LoadingState } from "@/ui/States";
import { Text } from "@/ui/Text";

type Discipline = components["schemas"]["Discipline"];

/** Rankings: the rolling 52-week points table per discipline (spec §12). */
export default function Rankings() {
  const styles = useStyles();
  const { features } = useCommunity();
  const me = useSignedIn().player_id;
  const { $api } = useApi();
  const disciplines = [
    { value: "singles" as const, label: "Singles" },
    ...(features.doubles ? [{ value: "doubles" as const, label: "Doubles" }] : []),
    ...(features.mixed ? [{ value: "mixed" as const, label: "Mixed" }] : []),
  ];
  const [discipline, setDiscipline] = useState<Discipline>("singles");
  const rankings = $api.useQuery("get", "/api/v1/rankings", {
    params: { query: { discipline, limit: 100 } },
  });
  const rows = rankings.data?.items ?? [];

  return (
    <Screen
      title="Rankings"
      refreshing={rankings.isRefetching}
      onRefresh={() => void rankings.refetch()}
    >
      <View style={{ gap: space.md }}>
        {disciplines.length > 1 ? (
          <Segmented options={disciplines} value={discipline} onChange={setDiscipline} />
        ) : null}
        <Button
          label="Your points history"
          variant="secondary"
          size="sm"
          icon="time-outline"
          onPress={() => router.push("/rankings/me")}
        />
        <Text variant="caption" tone="textMuted">
          Points from league and tournament play over the last 52 weeks.
          {rows[0] ? ` Updated ${formatDateTime(rows[0].refreshed_at)}.` : ""}
        </Text>
      </View>
      {rankings.isPending ? (
        <LoadingState />
      ) : rankings.error ? (
        <ErrorState error={rankings.error} onRetry={() => void rankings.refetch()} />
      ) : rows.length === 0 ? (
        <EmptyState
          icon="podium-outline"
          title="No points yet"
          body="League results earn ranking points. Play a league match to get on the board."
        />
      ) : (
        <View style={styles.table}>
          {rows.map((row) => {
            const mine = row.player_id === me;
            return (
              <Pressable
                key={row.player_id}
                accessibilityRole="button"
                accessibilityLabel={`Rank ${row.rank}, ${row.display_name}, ${row.points_52w} points`}
                onPress={() =>
                  router.push({ pathname: "/players/[id]", params: { id: row.player_id } })
                }
                style={({ pressed }) => [
                  styles.row,
                  mine && styles.mine,
                  pressed && styles.pressed,
                ]}
              >
                <Text variant="subheading" style={styles.rank}>
                  {row.rank}
                </Text>
                <Avatar id={row.player_id} name={row.display_name} size={32} />
                <Text
                  variant="body"
                  weight={mine ? "semibold" : "regular"}
                  style={styles.name}
                  numberOfLines={1}
                >
                  {mine ? `${row.display_name} (you)` : row.display_name}
                </Text>
                <Text variant="subheading" style={styles.points}>
                  {row.points_52w}
                </Text>
              </Pressable>
            );
          })}
        </View>
      )}
    </Screen>
  );
}

const useStyles = createStyles(({ colors }) => ({
  table: {
    backgroundColor: colors.surface,
    borderRadius: radius.lg,
    borderWidth: 1,
    borderColor: colors.border,
    overflow: "hidden",
  },
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: space.md,
    paddingHorizontal: space.lg,
    paddingVertical: space.md,
    borderTopWidth: 1,
    borderTopColor: colors.border,
    marginTop: -1,
  },
  mine: { backgroundColor: colors.primarySoft },
  pressed: { backgroundColor: colors.surfaceMuted },
  rank: { width: 28, textAlign: "center", fontVariant: ["tabular-nums"] },
  name: { flex: 1 },
  points: { fontVariant: ["tabular-nums"] },
}));
