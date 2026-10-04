import { router, useLocalSearchParams } from "expo-router";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { refreshAfterWrite } from "@/api/queryClient";
import { formatDateTime } from "@/features/format";
import { startOfDay } from "@/features/matches/when";
import { WhenPicker } from "@/features/matches/WhenPicker";
import { space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
import { Chip } from "@/ui/Chip";
import { Screen } from "@/ui/Screen";
import { ErrorState, LoadingState } from "@/ui/States";
import { Text } from "@/ui/Text";
import { TextField } from "@/ui/TextField";

/**
 * Challenge a player to a singles friendly, optionally with a first proposed time and place
 * (`POST /matches`). Doubles friendlies start from a match request instead.
 */
export default function Challenge() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const { $api } = useApi();
  const queryClient = useQueryClient();
  const opponent = $api.useQuery("get", "/api/v1/players/{id}", { params: { path: { id } } });
  const me = $api.useQuery("get", "/api/v1/me");
  const [now] = useState(() => new Date());
  const [day, setDay] = useState(() => startOfDay(now));
  const [time, setTime] = useState<Date | null>(null);
  const [location, setLocation] = useState("");
  const create = $api.useMutation("post", "/api/v1/matches", {
    onSuccess: async (match) => {
      await refreshAfterWrite(queryClient);
      router.replace({ pathname: "/matches/[id]", params: { id: match.id } });
    },
  });

  if (opponent.isPending) return <LoadingState />;
  if (opponent.error) {
    return <ErrorState error={opponent.error} onRetry={() => void opponent.refetch()} />;
  }
  const places = [
    ...new Set([
      ...(me.data?.player.preferred_locations ?? []),
      ...opponent.data.preferred_locations,
    ]),
  ];

  return (
    <Screen>
      <Text variant="title" accessibilityRole="header">
        Play {opponent.data.display_name}
      </Text>
      <Text tone="textMuted">
        A singles friendly. Suggest a time now, or arrange it together on the match page.
      </Text>
      <WhenPicker
        day={day}
        time={time}
        now={now}
        onDay={(next) => {
          setDay(next);
          setTime(null);
        }}
        onTime={setTime}
      />
      <View style={{ gap: space.sm }}>
        <TextField
          label="Where (optional)"
          value={location}
          onChangeText={setLocation}
          placeholder="Club, court or park"
        />
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: space.sm }}>
          {places.map((place) => (
            <Chip
              key={place}
              label={place}
              selected={place === location}
              onPress={() => setLocation(place)}
            />
          ))}
        </View>
      </View>
      {create.error ? <Text tone="danger">{describeError(create.error)}</Text> : null}
      <Button
        label={time ? `Challenge for ${formatDateTime(time.toISOString())}` : "Challenge"}
        block
        loading={create.isPending}
        onPress={() =>
          create.mutate({
            body: {
              discipline: "singles",
              opponent_ids: [id],
              proposed_time: time?.toISOString() ?? null,
              location: location.trim() || null,
            },
          })
        }
      />
    </Screen>
  );
}
