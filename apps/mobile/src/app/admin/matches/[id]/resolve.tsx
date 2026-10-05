import { useQueryClient } from "@tanstack/react-query";
import { useLocalSearchParams } from "expo-router";
import { useState } from "react";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { refreshAfterWrite } from "@/api/queryClient";
import { formatScore } from "@/features/matches/match";
import { ScoreForm } from "@/features/matches/ScoreForm";
import { checkRows, scoreRows, type RowInput } from "@/features/matches/scoreForm";
import { closeModal } from "@/features/navigation";
import { nameLookup, sideName } from "@/features/players/names";
import { useSignedIn } from "@/session/SessionProvider";
import { Button } from "@/ui/Button";
import { Card } from "@/ui/Card";
import { Screen } from "@/ui/Screen";
import { ErrorState, LoadingState } from "@/ui/States";
import { Text } from "@/ui/Text";
import { TextField } from "@/ui/TextField";

/** An admin settles a disputed match with the score that stands (decision 86's checks apply). */
export default function ResolveWithScore() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const me = useSignedIn().player_id;
  const { $api } = useApi();
  const queryClient = useQueryClient();
  const match = $api.useQuery("get", "/api/v1/matches/{id}", { params: { path: { id } } });
  const [inputs, setInputs] = useState<RowInput[]>([]);
  const [note, setNote] = useState("");
  const resolve = $api.useMutation("post", "/api/v1/admin/matches/{id}/resolve", {
    onSuccess: async () => {
      await refreshAfterWrite(queryClient);
      closeModal({ pathname: "/matches/[id]", params: { id } });
    },
  });

  if (match.isPending) return <LoadingState />;
  if (match.error) return <ErrorState error={match.error} onRetry={() => void match.refetch()} />;
  const data = match.data;
  const name = nameLookup(data.names, me);
  const sides = { a: sideName(data.side_a, name), b: sideName(data.side_b, name) };
  const rows = scoreRows(data.match_format, inputs);
  const verdict = checkRows(data.match_format, rows);
  const check = verdict?.check;
  const reported = data.score ? formatScore(data.score) : null;

  return (
    <Screen>
      {reported ? (
        <Text tone="textMuted">
          Reported: {sides.a} {reported} {sides.b}
          {data.dispute_note ? `. Disputed: “${data.dispute_note}”` : "."}
        </Text>
      ) : null}
      <ScoreForm
        format={data.match_format}
        rows={rows}
        sides={sides}
        first="a"
        onChange={(index, value) =>
          setInputs((current) => {
            const next = [...current];
            next[index] = value;
            return next;
          })
        }
      />
      {check?.ok ? (
        <Card>
          <Text variant="subheading">
            {sides[check.winner]} won {formatScore({ sets: verdict?.sets ?? [] }, check.winner)}
          </Text>
        </Card>
      ) : check ? (
        <Text tone="danger">{check.message}</Text>
      ) : null}
      <TextField
        label="Note for the players (optional)"
        value={note}
        onChangeText={setNote}
        multiline
      />
      {resolve.error ? <Text tone="danger">{describeError(resolve.error)}</Text> : null}
      <Button
        label="Settle with this score"
        block
        disabled={!check?.ok}
        loading={resolve.isPending}
        onPress={() =>
          verdict &&
          resolve.mutate({
            params: { path: { id } },
            body: {
              resolution: "score",
              score: { sets: verdict.sets },
              note: note.trim() || null,
            },
          })
        }
      />
    </Screen>
  );
}
