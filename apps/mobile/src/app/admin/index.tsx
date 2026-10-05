import { router } from "expo-router";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { unwrap, useCursorList } from "@/api/paging";
import { ModerationButton } from "@/features/admin/ModerationButton";
import { isAdmin } from "@/features/admin/roles";
import { formatDate } from "@/features/format";
import { MatchCard } from "@/features/matches/MatchCard";
import { nameLookup } from "@/features/players/names";
import { PlayerRow } from "@/features/players/PlayerRow";
import { useSignedIn } from "@/session/SessionProvider";
import { space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
import { Card } from "@/ui/Card";
import { Screen, Section } from "@/ui/Screen";
import { EmptyState, ErrorState, LoadingState } from "@/ui/States";
import { Text } from "@/ui/Text";

/** Club admin: what the club's admins look after, one section per job. */
export default function Admin() {
  const session = useSignedIn();
  if (!isAdmin(session.role)) {
    return (
      <EmptyState
        icon="lock-closed-outline"
        title="For club admins"
        body="Ask your club’s owner if you should help run it."
      />
    );
  }
  return (
    <Screen>
      <Leagues />
      <Disputes />
      <BannedMembers />
    </Screen>
  );
}

/** Starting a league, and the drafts waiting to be published. */
function Leagues() {
  const { $api } = useApi();
  const drafts = $api.useQuery("get", "/api/v1/leagues", {
    params: { query: { status: "draft", limit: 50 } },
  });
  return (
    <Section title="Leagues">
      <Button
        label="Create a league"
        icon="add-outline"
        onPress={() => router.push("/admin/leagues/new")}
      />
      {(drafts.data?.items ?? []).map((league) => (
        <Card
          key={league.id}
          accessibilityLabel={`${league.name}, ${league.published_at ? "published" : "draft"}`}
          onPress={() => router.push({ pathname: "/leagues/[id]", params: { id: league.id } })}
        >
          <Text variant="subheading">{league.name}</Text>
          <Text variant="label" tone="textMuted">
            {league.published_at
              ? `Published · registration opens ${formatDate(league.registration_opens_at)}`
              : "Draft · not published"}
          </Text>
        </Card>
      ))}
    </Section>
  );
}

/** Disputed matches across the club, each waiting for an admin's decision. */
function Disputes() {
  const me = useSignedIn().player_id;
  const { $api } = useApi();
  const disputed = $api.useQuery("get", "/api/v1/matches", {
    params: { query: { all: true, status: "disputed", limit: 100 } },
  });
  const items = disputed.data?.items ?? [];
  const name = nameLookup(
    items.flatMap((match) => match.names),
    me,
  );
  return (
    <Section title="Disputes">
      {disputed.isPending ? (
        <LoadingState />
      ) : disputed.error ? (
        <ErrorState error={disputed.error} onRetry={() => void disputed.refetch()} />
      ) : items.length === 0 ? (
        <Text tone="textMuted">No disputed matches.</Text>
      ) : (
        items.map((match) => (
          <MatchCard
            key={match.id}
            match={match}
            me={me}
            name={name}
            now={new Date(disputed.dataUpdatedAt)}
          />
        ))
      )}
    </Section>
  );
}

/** Banned members, each a tap from their profile or from lifting the ban. */
function BannedMembers() {
  const { fetch } = useApi();
  const banned = useCursorList(
    ["get", "/api/v1/players", { params: { query: { status: "banned" } } }],
    async (cursor, signal) =>
      unwrap(
        await fetch.GET("/api/v1/players", {
          params: { query: { status: "banned", cursor, limit: 30 } },
          signal,
        }),
      ),
  );
  return (
    <Section title="Banned members">
      <Text variant="caption" tone="textMuted">
        To ban someone, open their profile from Play → Players or a match.
      </Text>
      {banned.isPending ? (
        <LoadingState />
      ) : banned.error ? (
        <ErrorState error={banned.error} onRetry={() => void banned.refetch()} />
      ) : banned.items.length === 0 ? (
        <Text tone="textMuted">Nobody is banned.</Text>
      ) : (
        <>
          {banned.items.map((player) => (
            <View key={player.id} style={{ gap: space.sm }}>
              <PlayerRow player={player} />
              <View style={{ alignItems: "flex-start" }}>
                <ModerationButton player={player} action="unban" />
              </View>
            </View>
          ))}
          {banned.hasNextPage ? (
            <Button
              label="Show more"
              variant="ghost"
              loading={banned.isFetchingNextPage}
              onPress={() => void banned.fetchNextPage()}
            />
          ) : null}
        </>
      )}
    </Section>
  );
}
