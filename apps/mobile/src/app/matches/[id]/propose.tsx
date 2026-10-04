import { useQueryClient } from "@tanstack/react-query";
import { router, useLocalSearchParams } from "expo-router";
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

/** Propose a time and place; the other side accepts, declines or counter-proposes. */
export default function ProposeTime() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const { $api } = useApi();
  const queryClient = useQueryClient();
  const match = $api.useQuery("get", "/api/v1/matches/{id}", { params: { path: { id } } });
  const me = $api.useQuery("get", "/api/v1/me");
  const [now] = useState(() => new Date());
  const [day, setDay] = useState(() => startOfDay(now));
  const [time, setTime] = useState<Date | null>(null);
  const [location, setLocation] = useState<string | null>(null);
  const propose = $api.useMutation("post", "/api/v1/matches/{id}/proposals", {
    onSuccess: async () => {
      await refreshAfterWrite(queryClient);
      router.back();
    },
  });

  if (match.isPending) return <LoadingState />;
  if (match.error) return <ErrorState error={match.error} onRetry={() => void match.refetch()} />;
  // Until the player types, the place defaults to where the match was last arranged.
  const place = location ?? match.data.location ?? "";
  const suggestions = [
    ...new Set(
      [match.data.location, ...(me.data?.player.preferred_locations ?? [])].filter(
        (value): value is string => !!value,
      ),
    ),
  ];

  return (
    <Screen>
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
          label="Where"
          value={place}
          onChangeText={setLocation}
          placeholder="Club, court or park"
        />
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: space.sm }}>
          {suggestions.map((suggestion) => (
            <Chip
              key={suggestion}
              label={suggestion}
              selected={suggestion === place}
              onPress={() => setLocation(suggestion)}
            />
          ))}
        </View>
      </View>
      {propose.error ? <Text tone="danger">{describeError(propose.error)}</Text> : null}
      <Button
        label={time ? `Propose ${formatDateTime(time.toISOString())}` : "Pick a start time"}
        block
        disabled={!time}
        loading={propose.isPending}
        onPress={() =>
          time &&
          propose.mutate({
            params: { path: { id } },
            body: { time: time.toISOString(), location: place.trim() || null },
          })
        }
      />
    </Screen>
  );
}
