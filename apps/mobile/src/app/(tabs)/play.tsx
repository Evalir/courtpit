import { useState } from "react";
import { ActivityIndicator, FlatList, View } from "react-native";

import { useApi } from "@/api/client";
import { unwrap, useCursorList } from "@/api/paging";
import { PlayerRow } from "@/features/players/PlayerRow";
import { useDebounced } from "@/features/useDebounced";
import { useTheme } from "@/theme/ThemeProvider";
import { space } from "@/theme/tokens";
import { ScreenTitle, useScreenStyles, useTitleInset } from "@/ui/Screen";
import { EmptyState, ErrorState } from "@/ui/States";
import { TextField } from "@/ui/TextField";

/**
 * Play: the player directory (spec §11). Open match requests and proposing a friendly join
 * this tab next (docs/frontend.md, Build order).
 */
export default function Play() {
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
      ListHeaderComponent={
        <View style={{ gap: space.md, marginBottom: space.lg }}>
          <ScreenTitle title="Find a player" />
          <TextField
            label="Search by name"
            value={search}
            onChangeText={setSearch}
            placeholder="e.g. Ana"
            autoCorrect={false}
            returnKeyType="search"
          />
        </View>
      }
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
