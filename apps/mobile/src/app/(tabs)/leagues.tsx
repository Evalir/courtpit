import { router } from "expo-router";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { disciplineLabel } from "@/features/format";
import { leagueBadge, leagueOrder, leagueTiming, type LeagueView } from "@/features/leagues/league";
import { space } from "@/theme/tokens";
import { Badge } from "@/ui/Badge";
import { Card } from "@/ui/Card";
import { Screen, Section } from "@/ui/Screen";
import { EmptyState, ErrorState, LoadingState } from "@/ui/States";
import { Text } from "@/ui/Text";

const SECTION_TITLE: Record<LeagueView["status"], string> = {
  registration: "Open for entries",
  active: "In season",
  draft: "Drafts (admins only)",
  finished: "Past seasons",
  cancelled: "Cancelled",
};

/** Leagues: every published season, grouped by where it stands. */
export default function Leagues() {
  const { $api } = useApi();
  const leagues = $api.useQuery("get", "/api/v1/leagues", { params: { query: { limit: 100 } } });

  if (leagues.isPending) return <LoadingState />;
  if (leagues.error)
    return <ErrorState error={leagues.error} onRetry={() => void leagues.refetch()} />;
  const now = new Date(leagues.dataUpdatedAt);
  const items = leagues.data.items;

  return (
    <Screen
      title="Leagues"
      refreshing={leagues.isRefetching}
      onRefresh={() => void leagues.refetch()}
    >
      {items.length === 0 ? (
        <EmptyState
          icon="trophy-outline"
          title="No leagues yet"
          body="When your club opens a season, it shows up here."
        />
      ) : (
        leagueOrder.map((status) => {
          const group = items.filter((league) => league.status === status);
          return group.length > 0 ? (
            <Section key={status} title={SECTION_TITLE[status]}>
              {group.map((league) => (
                <LeagueCard key={league.id} league={league} now={now} />
              ))}
            </Section>
          ) : null;
        })
      )}
    </Screen>
  );
}

function LeagueCard({ league, now }: { league: LeagueView; now: Date }) {
  const badge = leagueBadge[league.status];
  return (
    <Card
      accessibilityLabel={`${league.name}, ${badge.label}`}
      onPress={() => router.push({ pathname: "/leagues/[id]", params: { id: league.id } })}
    >
      <View style={{ flexDirection: "row", gap: space.sm, alignItems: "center" }}>
        <Badge label={badge.label} tone={badge.tone} />
        <Text variant="caption" tone="textMuted">
          {disciplineLabel[league.discipline]}
        </Text>
      </View>
      <Text variant="heading">{league.name}</Text>
      <Text variant="label" tone="textMuted">
        {leagueTiming(league, now)}
      </Text>
    </Card>
  );
}
