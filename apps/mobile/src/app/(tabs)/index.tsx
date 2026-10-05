import { router } from "expo-router";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { needsYouLine } from "@/features/home";
import { entriesNeedingYou, entryState, type MyEntry } from "@/features/leagues/entries";
import { InvitationCard } from "@/features/leagues/InvitationCard";
import { PushPrompt } from "@/features/notifications/PushPrompt";
import { groupMatches, type MatchGroups, type MatchView } from "@/features/matches/match";
import { MatchCard } from "@/features/matches/MatchCard";
import { nameLookup } from "@/features/players/names";
import { useSignedIn } from "@/session/SessionProvider";
import { useCommunity } from "@/tenant/TenantProvider";
import { space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
import { Card } from "@/ui/Card";
import { Screen, Section } from "@/ui/Screen";
import { EmptyState, ErrorState, LoadingState } from "@/ui/States";
import { Text } from "@/ui/Text";

const SECTIONS: { key: keyof MatchGroups; title: string; limit?: number }[] = [
  { key: "needsYou", title: "Needs you" },
  { key: "upcoming", title: "Upcoming" },
  { key: "toArrange", title: "To arrange" },
  { key: "waiting", title: "Waiting on others" },
  { key: "results", title: "Recent results", limit: 5 },
];

/** Home: the signed-in player's matches, sorted by what needs doing. */
export default function Home() {
  const session = useSignedIn();
  const community = useCommunity();
  const me = session.player_id;
  const { $api } = useApi();
  const profile = $api.useQuery("get", "/api/v1/me");
  const matches = $api.useQuery("get", "/api/v1/matches", { params: { query: { limit: 100 } } });
  const entries = $api.useQuery("get", "/api/v1/me/entries");
  const items: MatchView[] = matches.data?.items ?? [];
  const name = nameLookup(
    items.flatMap((match) => match.names),
    me,
  );

  if (matches.isPending) return <LoadingState />;
  if (matches.error)
    return <ErrorState error={matches.error} onRetry={() => void matches.refetch()} />;

  // "Now" as of the data, so the grouping is a pure function of what was fetched.
  const now = new Date(matches.dataUpdatedAt);
  const groups = groupMatches(items, me, now);
  const leagueTodos = entriesNeedingYou(entries.data ?? [], me, now);
  const firstName = profile.data?.player.display_name.split(" ")[0];

  return (
    <Screen
      eyebrow={community.name}
      title={firstName ? `Hi, ${firstName}` : "Hi"}
      refreshing={matches.isRefetching || entries.isRefetching}
      onRefresh={() => {
        void matches.refetch();
        void entries.refetch();
      }}
    >
      <View style={{ marginTop: -space.lg }}>
        <Text tone="textMuted">{needsYouLine(groups.needsYou.length, leagueTodos.length)}</Text>
      </View>
      <PushPrompt />
      {leagueTodos.length > 0 ? (
        <Section title="Leagues">
          {leagueTodos.map((item) => (
            <LeagueTodo key={item.entry.id} item={item} me={me} />
          ))}
        </Section>
      ) : null}
      {items.length === 0 ? (
        <EmptyState
          icon="tennisball-outline"
          title="No matches yet"
          body="Find someone at your level to play, or enter a league."
        >
          <Button label="Find a player" onPress={() => router.navigate("/play")} />
          <Button label="See leagues" variant="ghost" onPress={() => router.navigate("/leagues")} />
        </EmptyState>
      ) : (
        SECTIONS.map(({ key, title, limit }) =>
          groups[key].length > 0 ? (
            <Section key={key} title={title}>
              {groups[key].slice(0, limit).map((match) => (
                <MatchCard key={match.id} match={match} me={me} name={name} now={now} />
              ))}
            </Section>
          ) : null,
        )
      )}
    </Screen>
  );
}

/** An invitation to answer, or an entry whose partner fell through. */
function LeagueTodo({ item, me }: { item: MyEntry; me: string }) {
  const { entry, league } = item;
  const state = entryState(entry, me);
  if (state?.kind === "invited") {
    const from = nameLookup(entry.names, me)(state.by);
    return <InvitationCard entry={entry} from={from} league={league.name} />;
  }
  return (
    <Card
      accessibilityLabel={`${league.name}: your entry needs a partner`}
      onPress={() => router.push({ pathname: "/leagues/[id]", params: { id: league.id } })}
    >
      <Text variant="subheading">{league.name}: your entry needs a partner</Text>
      <Text variant="label" tone="textMuted">
        Your invitation was declined, or your partner entered with someone else. Invite someone else
        or list yourself as looking.
      </Text>
    </Card>
  );
}
