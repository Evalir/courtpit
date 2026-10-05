import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { describeError, errorCode } from "@/api/errors";
import { refreshAfterWrite } from "@/api/queryClient";
import { formatLastDay } from "@/features/format";
import type { LeagueView } from "@/features/leagues/league";
import { MatchCard } from "@/features/matches/MatchCard";
import { nameLookup } from "@/features/players/names";
import { useSignedIn } from "@/session/SessionProvider";
import { space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
import { Chip } from "@/ui/Chip";
import { Section } from "@/ui/Screen";
import { Text } from "@/ui/Text";

import { isAdmin } from "./roles";
import { canFinish, soloEntries, togglePick } from "./season";

/** Running a season, for admins: pair solo entries before the draw, then close it out. */
export function SeasonAdmin({ league, now }: { league: LeagueView; now: Date }) {
  const session = useSignedIn();
  if (!isAdmin(session.role)) return null;
  if (league.status === "registration" && league.discipline !== "singles") {
    return <Pairing league={league} />;
  }
  if (league.status === "active") return <ClosingOut league={league} now={now} />;
  return null;
}

/** Solo entries an admin can pair, two at a time. */
function Pairing({ league }: { league: LeagueView }) {
  const me = useSignedIn().player_id;
  const { $api } = useApi();
  const queryClient = useQueryClient();
  const [picked, setPicked] = useState<string[]>([]);
  const entries = $api.useQuery("get", "/api/v1/leagues/{id}/entries", {
    params: { path: { id: league.id } },
  });
  const pair = $api.useMutation("post", "/api/v1/admin/leagues/{id}/pair", {
    onSuccess: async () => {
      setPicked([]);
      await refreshAfterWrite(queryClient);
    },
  });
  const solos = soloEntries(entries.data ?? []);
  const name = nameLookup(
    (entries.data ?? []).flatMap((entry) => entry.names),
    me,
  );
  if (solos.length < 2) return null;
  const [first, second] = picked;
  const label = (id: string) => {
    const entry = solos.find((solo) => solo.id === id);
    return entry ? name(entry.created_by) : "";
  };

  return (
    <Section title="Pair solo entries">
      <Text variant="caption" tone="textMuted">
        Players without a partner yet. Pick two to enter them together; the first keeps the entry.
      </Text>
      <View style={{ flexDirection: "row", flexWrap: "wrap", gap: space.sm }}>
        {solos.map((entry) => (
          <Chip
            key={entry.id}
            label={`${name(entry.created_by)}${entry.looking_for_partner ? "" : " (invited someone)"}`}
            selected={picked.includes(entry.id)}
            onPress={() => setPicked(togglePick(picked, entry.id))}
          />
        ))}
      </View>
      {pair.error ? <Text tone="danger">{describeError(pair.error)}</Text> : null}
      <Button
        label={first && second ? `Pair ${label(first)} & ${label(second)}` : "Pick two players"}
        disabled={!first || !second}
        loading={pair.isPending}
        onPress={() =>
          first &&
          second &&
          pair.mutate({ params: { path: { id: league.id } }, body: { entry_ids: [first, second] } })
        }
      />
    </Section>
  );
}

/** The matches still without a result, and finishing the season once it is over. */
function ClosingOut({ league, now }: { league: LeagueView; now: Date }) {
  const me = useSignedIn().player_id;
  const { $api } = useApi();
  const queryClient = useQueryClient();
  const [force, setForce] = useState(false);
  const unresolved = $api.useQuery("get", "/api/v1/admin/leagues/{id}/unresolved", {
    params: { path: { id: league.id }, query: { limit: 50 } },
  });
  const finish = $api.useMutation("post", "/api/v1/admin/leagues/{id}/finish", {
    onSuccess: () => refreshAfterWrite(queryClient),
  });
  const items = unresolved.data?.items ?? [];
  const name = nameLookup(
    items.flatMap((match) => match.names),
    me,
  );
  const over = canFinish(league, now);
  const blocked = errorCode(finish.error) === "unresolved_matches";

  return (
    <Section title="Closing the season">
      {items.length > 0 ? (
        <>
          <Text variant="caption" tone="textMuted">
            Waiting for a result: reported scores confirm themselves; disputes need an admin.
          </Text>
          {items.map((match) => (
            <MatchCard key={match.id} match={match} me={me} name={name} now={now} />
          ))}
        </>
      ) : (
        <Text variant="caption" tone="textMuted">
          Every played match has its result.
        </Text>
      )}
      {!over ? (
        <Text variant="label" tone="textMuted">
          The season runs until {formatLastDay(league.ends_at)}; it can be finished after that.
        </Text>
      ) : blocked || force ? (
        <View style={{ gap: space.sm }}>
          <Text variant="label">
            Some matches still wait for a result. Finish anyway and leave them out of the final
            table?
          </Text>
          <View style={{ flexDirection: "row", gap: space.sm }}>
            <Button
              label="Finish anyway"
              variant="danger"
              size="sm"
              loading={finish.isPending}
              onPress={() =>
                finish.mutate({ params: { path: { id: league.id }, query: { force: true } } })
              }
            />
            <Button
              label="Back"
              variant="ghost"
              size="sm"
              onPress={() => {
                setForce(false);
                finish.reset();
              }}
            />
          </View>
        </View>
      ) : (
        <>
          {finish.error ? <Text tone="danger">{describeError(finish.error)}</Text> : null}
          <Button
            label="Finish the season"
            loading={finish.isPending}
            onPress={() =>
              items.length > 0
                ? setForce(true)
                : finish.mutate({ params: { path: { id: league.id }, query: {} } })
            }
          />
          <Text variant="caption" tone="textMuted">
            Finishing writes the final table, awards season points and sets up promotion.
          </Text>
        </>
      )}
    </Section>
  );
}
