import type { components } from "@courtpit/api-client";
import { useLocalSearchParams } from "expo-router";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { disciplineLabel, formatDate, formatLastDay } from "@/features/format";
import { AdminLeagueActions } from "@/features/admin/AdminLeagueActions";
import { SeasonAdmin } from "@/features/admin/SeasonAdmin";
import { EntryPanel } from "@/features/leagues/EntryPanel";
import { describeFormat, leagueBadge, leagueTiming, ownBoxFirst } from "@/features/leagues/league";
import { nameLookup, sideName } from "@/features/players/names";
import { useSignedIn } from "@/session/SessionProvider";
import { createStyles } from "@/theme/ThemeProvider";
import { radius, space } from "@/theme/tokens";
import { Badge } from "@/ui/Badge";
import { Card } from "@/ui/Card";
import { Screen, Section } from "@/ui/Screen";
import { ErrorState, LoadingState } from "@/ui/States";
import { Text } from "@/ui/Text";

type DivisionStanding = components["schemas"]["DivisionStanding"];

/**
 * A league: its dates and format; registration while it takes entries; then each box's table
 * once the season has started, the viewer's box first.
 */
export default function League() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const { $api } = useApi();
  const me = useSignedIn().player_id;
  const league = $api.useQuery("get", "/api/v1/leagues/{id}", { params: { path: { id } } });
  const started = league.data?.status === "active" || league.data?.status === "finished";
  const standings = $api.useQuery(
    "get",
    "/api/v1/leagues/{id}/standings",
    { params: { path: { id } } },
    { enabled: started },
  );
  const name = nameLookup(
    (standings.data ?? []).flatMap((box) => box.table.flatMap((line) => line.names)),
    me,
  );

  if (league.isPending) return <LoadingState />;
  if (league.error)
    return <ErrorState error={league.error} onRetry={() => void league.refetch()} />;
  const data = league.data;
  const badge = leagueBadge[data.status];
  const now = new Date(league.dataUpdatedAt);
  const boxes = ownBoxFirst(standings.data ?? [], me);

  return (
    <Screen
      refreshing={league.isRefetching || standings.isRefetching}
      onRefresh={() => {
        void league.refetch();
        if (started) void standings.refetch();
      }}
    >
      <View style={{ gap: space.sm }}>
        <View style={{ flexDirection: "row", gap: space.sm, alignItems: "center" }}>
          <Badge label={badge.label} tone={badge.tone} />
          <Text variant="caption" tone="textMuted">
            {disciplineLabel[data.discipline]}
          </Text>
        </View>
        <Text variant="title" accessibilityRole="header">
          {data.name}
        </Text>
        <Text tone="textMuted">{leagueTiming(data, now)}</Text>
      </View>
      <Card>
        <Fact
          label="Registration"
          value={`${formatDate(data.registration_opens_at)} – ${formatLastDay(data.registration_closes_at)}`}
        />
        <Fact
          label="Season"
          value={`${formatDate(data.starts_at)} – ${formatLastDay(data.ends_at)}`}
        />
        <Fact label="Format" value={describeFormat(data.match_format)} />
        <Fact
          label="Boxes"
          value={`${data.box_min_size}–${data.box_max_size} entries, round robin`}
        />
      </Card>
      <AdminLeagueActions league={data} />
      <SeasonAdmin league={data} now={now} />
      <EntryPanel league={data} now={now} />
      {started ? (
        standings.isPending ? (
          <LoadingState />
        ) : standings.error ? (
          <ErrorState error={standings.error} onRetry={() => void standings.refetch()} />
        ) : (
          boxes.boxes.map((box) => (
            <BoxTable
              key={box.division_id}
              box={box}
              mine={box === boxes.mine}
              me={me}
              name={name}
            />
          ))
        )
      ) : null}
    </Screen>
  );
}

function Fact({ label, value }: { label: string; value: string }) {
  return (
    <View style={{ gap: space.xxs }}>
      <Text variant="caption" tone="textMuted">
        {label}
      </Text>
      <Text variant="label">{value}</Text>
    </View>
  );
}

/** One box: position, entry, played / won / lost and table points, the viewer highlighted. */
function BoxTable({
  box,
  mine,
  me,
  name,
}: {
  box: DivisionStanding;
  mine: boolean;
  me: string;
  name: (id: string) => string;
}) {
  const styles = useStyles();
  return (
    <Section title={`${mine ? "Your box · " : ""}${box.name} · Tier ${box.tier}`}>
      <View style={styles.table} accessibilityRole="list">
        <View style={[styles.row, styles.head]}>
          <Text variant="caption" tone="textMuted" style={styles.pos}>
            #
          </Text>
          <Text variant="caption" tone="textMuted" style={styles.who}>
            Player
          </Text>
          {["P", "W", "L", "Pts"].map((column) => (
            <Text key={column} variant="caption" tone="textMuted" style={styles.num}>
              {column}
            </Text>
          ))}
        </View>
        {box.table.map((line) => {
          const mine = line.player_ids.includes(me);
          return (
            <View key={line.entry_id} style={[styles.row, mine && styles.mine]}>
              <Text variant="label" weight="semibold" style={styles.pos}>
                {line.position}
              </Text>
              <Text
                variant="label"
                weight={mine ? "semibold" : "regular"}
                style={styles.who}
                numberOfLines={1}
              >
                {sideName(line.player_ids, name)}
              </Text>
              {[line.played, line.won, line.lost].map((value, index) => (
                <Text key={index} variant="label" tone="textMuted" style={styles.num}>
                  {value}
                </Text>
              ))}
              <Text variant="label" weight="bold" style={styles.num}>
                {line.points}
              </Text>
            </View>
          );
        })}
      </View>
    </Section>
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
    paddingHorizontal: space.md,
    paddingVertical: space.sm + 2,
    borderTopWidth: 1,
    borderTopColor: colors.border,
  },
  head: { borderTopWidth: 0, paddingVertical: space.sm },
  mine: { backgroundColor: colors.primarySoft },
  pos: { width: 28, fontVariant: ["tabular-nums"] },
  who: { flex: 1, paddingRight: space.sm },
  num: { width: 34, textAlign: "right", fontVariant: ["tabular-nums"] },
}));
