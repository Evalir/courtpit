import { View } from "react-native";

import { useApi } from "@/api/client";
import { unwrap, useCursorList } from "@/api/paging";
import { ModerationButton } from "@/features/admin/ModerationButton";
import { isAdmin } from "@/features/admin/roles";
import { PlayerRow } from "@/features/players/PlayerRow";
import { useSignedIn } from "@/session/SessionProvider";
import { space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
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
      <BannedMembers />
    </Screen>
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
