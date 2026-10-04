import { useQueryClient } from "@tanstack/react-query";
import { router, useLocalSearchParams } from "expo-router";
import { useState } from "react";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { refreshAfterWrite } from "@/api/queryClient";
import { formatScore, sideOf } from "@/features/matches/match";
import { ScoreForm } from "@/features/matches/ScoreForm";
import { checkRows, scoreRows, type RowInput } from "@/features/matches/scoreForm";
import { nameLookup, sideName } from "@/features/players/names";
import { useSignedIn } from "@/session/SessionProvider";
import { Button } from "@/ui/Button";
import { Card } from "@/ui/Card";
import { Screen } from "@/ui/Screen";
import { ErrorState, LoadingState } from "@/ui/States";
import { Text } from "@/ui/Text";

/**
 * Report the score set by set. The form follows the match format and checks the score the way
 * the server does (decision 86) before it is sent.
 */
export default function ReportScore() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const me = useSignedIn().player_id;
  const { $api } = useApi();
  const queryClient = useQueryClient();
  const match = $api.useQuery("get", "/api/v1/matches/{id}", { params: { path: { id } } });
  const [inputs, setInputs] = useState<RowInput[]>([]);
  const report = $api.useMutation("post", "/api/v1/matches/{id}/score", {
    onSuccess: async () => {
      await refreshAfterWrite(queryClient);
      router.back();
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

  return (
    <Screen>
      <ScoreForm
        format={data.match_format}
        rows={rows}
        sides={sides}
        first={sideOf(data, me) ?? "a"}
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
            {sides[check.winner]} {sides[check.winner] === "You" ? "win" : "won"}{" "}
            {formatScore({ sets: verdict?.sets ?? [] }, check.winner)}
          </Text>
          <Text variant="label" tone="textMuted">
            The other side confirms or disputes it; unanswered scores confirm themselves after a few
            days.
          </Text>
        </Card>
      ) : check ? (
        <Text tone="danger">{check.message}</Text>
      ) : null}
      {report.error ? <Text tone="danger">{describeError(report.error)}</Text> : null}
      <Button
        label="Report score"
        block
        disabled={!check?.ok}
        loading={report.isPending}
        onPress={() =>
          verdict && report.mutate({ params: { path: { id } }, body: { sets: verdict.sets } })
        }
      />
    </Screen>
  );
}
