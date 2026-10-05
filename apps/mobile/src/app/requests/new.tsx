import type { components } from "@courtpit/api-client";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { refreshAfterWrite } from "@/api/queryClient";
import { formatUtr } from "@/features/format";
import { startOfDay } from "@/features/matches/when";
import { WhenPicker } from "@/features/matches/WhenPicker";
import { PlayerPicker } from "@/features/players/PlayerPicker";
import { bandAround, levelLabel, windowLabel } from "@/features/requests/request";
import { closeModal } from "@/features/navigation";
import { useCommunity } from "@/tenant/TenantProvider";
import { space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
import { Chip } from "@/ui/Chip";
import { Screen, Section } from "@/ui/Screen";
import { Segmented } from "@/ui/Segmented";
import { Text } from "@/ui/Text";
import { TextField } from "@/ui/TextField";

type Discipline = components["schemas"]["Discipline"];
type PlayerPublic = components["schemas"]["PlayerPublic"];

const LENGTHS = [60, 90, 120, 180] as const;

/** Open a request: when you can play, at what level, where; others join until it fills. */
export default function NewRequest() {
  const { features } = useCommunity();
  const { $api } = useApi();
  const queryClient = useQueryClient();
  const me = $api.useQuery("get", "/api/v1/me");
  const [now] = useState(() => new Date());
  const [discipline, setDiscipline] = useState<Discipline>("singles");
  const [partner, setPartner] = useState<PlayerPublic | null>(null);
  const [day, setDay] = useState(() => startOfDay(now));
  const [start, setStart] = useState<Date | null>(null);
  const [minutes, setMinutes] = useState<number>(120);
  const [band, setBand] = useState<{ min: number; max: number } | null>(null);
  const [location, setLocation] = useState("");
  const create = $api.useMutation("post", "/api/v1/match-requests", {
    onSuccess: async () => {
      await refreshAfterWrite(queryClient);
      closeModal("/play");
    },
  });

  const myUtr = me.data?.player.utr ?? null;
  const end = start ? new Date(start.getTime() + minutes * 60_000) : null;
  const disciplines = [
    { value: "singles" as const, label: "Singles" },
    ...(features.doubles ? [{ value: "doubles" as const, label: "Doubles" }] : []),
    ...(features.mixed ? [{ value: "mixed" as const, label: "Mixed" }] : []),
  ];

  return (
    <Screen>
      {disciplines.length > 1 ? (
        <Segmented
          options={disciplines}
          value={discipline}
          onChange={(next) => {
            setDiscipline(next);
            if (next === "singles") setPartner(null);
          }}
        />
      ) : null}
      {discipline !== "singles" ? (
        <View style={{ gap: space.xs }}>
          <PlayerPicker label="Your partner (optional)" value={partner} onChange={setPartner} />
          <Text variant="caption" tone="textMuted">
            {partner
              ? "You two look for a pair to play."
              : "Without a partner, the first player to join plays with you."}
          </Text>
        </View>
      ) : null}
      <WhenPicker
        day={day}
        time={start}
        now={now}
        onDay={(next) => {
          setDay(next);
          setStart(null);
        }}
        onTime={setStart}
      />
      <Section title="For how long">
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: space.sm }}>
          {LENGTHS.map((length) => (
            <Chip
              key={length}
              label={`${Math.floor(length / 60)}${length % 60 ? "½" : ""} h`}
              selected={minutes === length}
              onPress={() => setMinutes(length)}
            />
          ))}
        </View>
      </Section>
      <Section title="Level">
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: space.sm }}>
          <Chip label="Any level" selected={band === null} onPress={() => setBand(null)} />
          {myUtr != null ? (
            <Chip
              label={`Around mine (${formatUtr(myUtr)})`}
              selected={band !== null}
              onPress={() => setBand(bandAround(myUtr))}
            />
          ) : null}
        </View>
        {band ? (
          <Text variant="caption" tone="textMuted">
            {levelLabel(band.min, band.max)}
          </Text>
        ) : null}
      </Section>
      <View style={{ gap: space.sm }}>
        <TextField
          label="Where"
          value={location}
          onChangeText={setLocation}
          placeholder="Club, court or park"
        />
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: space.sm }}>
          {(me.data?.player.preferred_locations ?? []).map((place) => (
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
        label={
          start && end
            ? `Post for ${windowLabel(start.toISOString(), end.toISOString())}`
            : "Pick a start time"
        }
        block
        disabled={!start || !end}
        loading={create.isPending}
        onPress={() =>
          start &&
          end &&
          create.mutate({
            body: {
              discipline,
              partner_id: partner?.id ?? null,
              time_window_start: start.toISOString(),
              time_window_end: end.toISOString(),
              utr_min: band?.min ?? null,
              utr_max: band?.max ?? null,
              location: location.trim() || null,
            },
          })
        }
      />
    </Screen>
  );
}
