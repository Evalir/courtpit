import { router } from "expo-router";
import { useState, type ReactElement } from "react";
import { ActivityIndicator, FlatList, View } from "react-native";

import { useApi } from "@/api/client";
import { unwrap, useCursorList } from "@/api/paging";
import { PlayerRow } from "@/features/players/PlayerRow";
import { RequestCard } from "@/features/requests/RequestCard";
import { useDebounced } from "@/features/useDebounced";
import { useSignedIn } from "@/session/SessionProvider";
import { useCommunity } from "@/tenant/TenantProvider";
import { useTheme } from "@/theme/ThemeProvider";
import { space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
import { Chip } from "@/ui/Chip";
import { ScreenTitle, useScreenStyles, useTitleInset } from "@/ui/Screen";
import { Segmented } from "@/ui/Segmented";
import { EmptyState, ErrorState } from "@/ui/States";
import { TextField } from "@/ui/TextField";

type View_ = "requests" | "players";

/**
 * Play: open match requests to join or start (spec §11), and the player directory to find
 * someone and challenge them. Requests are hidden when the community turns them off.
 */
export default function Play() {
  const { features } = useCommunity();
  const [view, setView] = useState<View_>(features.matchRequests ? "requests" : "players");
  const header = (extra: ReactElement) => (
    <View style={{ gap: space.md, marginBottom: space.lg }}>
      <ScreenTitle title="Play" />
      {features.matchRequests ? (
        <Segmented
          options={[
            { value: "requests" as const, label: "Open requests" },
            { value: "players" as const, label: "Players" },
          ]}
          value={view}
          onChange={setView}
        />
      ) : null}
      {extra}
    </View>
  );
  return view === "requests" ? <Requests header={header} /> : <Directory header={header} />;
}

function Requests({ header }: { header: (extra: ReactElement) => ReactElement }) {
  const styles = useScreenStyles();
  const titleInset = useTitleInset();
  const { colors } = useTheme();
  const { fetch, $api } = useApi();
  const me = useSignedIn().player_id;
  const profile = $api.useQuery("get", "/api/v1/me");
  const [fitsMe, setFitsMe] = useState(false);
  const query = { fits_me: fitsMe || undefined };
  const requests = useCursorList(
    ["get", "/api/v1/match-requests", { params: { query } }],
    async (cursor, signal) =>
      unwrap(
        await fetch.GET("/api/v1/match-requests", {
          params: { query: { ...query, cursor, limit: 20 } },
          signal,
        }),
      ),
  );
  return (
    <FlatList
      style={styles.screen}
      contentContainerStyle={[styles.list, titleInset]}
      data={requests.items}
      keyExtractor={(request) => request.id}
      renderItem={({ item }) => (
        <RequestCard request={item} me={me} myUtr={profile.data?.player.utr} />
      )}
      ItemSeparatorComponent={() => <View style={styles.separator} />}
      onEndReached={() => {
        if (requests.hasNextPage && !requests.isFetchingNextPage) void requests.fetchNextPage();
      }}
      refreshing={requests.isRefetching && !requests.isFetchingNextPage}
      onRefresh={() => void requests.refetch()}
      ListHeaderComponent={header(
        <View style={{ flexDirection: "row", alignItems: "center", gap: space.sm }}>
          <Button
            label="New request"
            icon="add"
            size="sm"
            onPress={() => router.push("/requests/new")}
          />
          <Chip label="Fits my level" selected={fitsMe} onPress={() => setFitsMe(!fitsMe)} />
        </View>,
      )}
      ListEmptyComponent={
        requests.isPending ? (
          <ActivityIndicator color={colors.primary} />
        ) : requests.error ? (
          <ErrorState error={requests.error} onRetry={() => void requests.refetch()} />
        ) : (
          <EmptyState
            icon="megaphone-outline"
            title="No open requests"
            body="Say when you can play and let others join you."
          />
        )
      }
    />
  );
}

function Directory({ header }: { header: (extra: ReactElement) => ReactElement }) {
  const styles = useScreenStyles();
  const titleInset = useTitleInset();
  const { colors } = useTheme();
  const { fetch } = useApi();
  const [search, setSearch] = useState("");
  const q = useDebounced(search.trim()) || undefined;
  const players = useCursorList(
    ["get", "/api/v1/players", { params: { query: { q } } }],
    async (cursor, signal) =>
      unwrap(
        await fetch.GET("/api/v1/players", {
          params: { query: { q, cursor, limit: 30 } },
          signal,
        }),
      ),
  );
  return (
    <FlatList
      style={styles.screen}
      contentContainerStyle={[styles.list, titleInset]}
      data={players.items}
      keyExtractor={(player) => player.id}
      renderItem={({ item }) => <PlayerRow player={item} />}
      ItemSeparatorComponent={() => <View style={styles.separator} />}
      keyboardShouldPersistTaps="handled"
      onEndReached={() => {
        if (players.hasNextPage && !players.isFetchingNextPage) void players.fetchNextPage();
      }}
      onEndReachedThreshold={0.5}
      refreshing={players.isRefetching && !players.isFetchingNextPage}
      onRefresh={() => void players.refetch()}
      ListHeaderComponent={header(
        <TextField
          label="Search by name"
          value={search}
          onChangeText={setSearch}
          placeholder="e.g. Ana"
          autoCorrect={false}
          returnKeyType="search"
        />,
      )}
      ListEmptyComponent={
        players.isPending ? (
          <ActivityIndicator color={colors.primary} />
        ) : players.error ? (
          <ErrorState error={players.error} onRetry={() => void players.refetch()} />
        ) : (
          <EmptyState
            icon="people-outline"
            title={q ? `Nobody called “${q}”` : "No other players yet"}
            body={q ? "Try part of their first or last name." : "Invite your club to join."}
          />
        )
      }
      ListFooterComponent={
        players.isFetchingNextPage ? <ActivityIndicator color={colors.primary} /> : null
      }
    />
  );
}
