import { useQueryClient } from "@tanstack/react-query";
import { useLocalSearchParams } from "expo-router";
import { useState } from "react";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { refreshAfterWrite } from "@/api/queryClient";
import { LeagueFields } from "@/features/admin/LeagueFields";
import {
  checkLeague,
  formFromLeague,
  patchBody,
  type LeagueForm,
} from "@/features/admin/leagueForm";
import type { LeagueView } from "@/features/leagues/league";
import { closeModal } from "@/features/navigation";
import { Button } from "@/ui/Button";
import { Screen } from "@/ui/Screen";
import { ErrorState, LoadingState } from "@/ui/States";
import { Text } from "@/ui/Text";

/** Edit a draft league (published or not, until registration opens). */
export default function EditLeague() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const { $api } = useApi();
  const league = $api.useQuery("get", "/api/v1/leagues/{id}", { params: { path: { id } } });
  if (league.isPending) return <LoadingState />;
  if (league.error)
    return <ErrorState error={league.error} onRetry={() => void league.refetch()} />;
  return <Editor league={league.data} />;
}

function Editor({ league }: { league: LeagueView }) {
  const { $api } = useApi();
  const queryClient = useQueryClient();
  const [form, setForm] = useState(() => formFromLeague(league));
  const [tried, setTried] = useState(false);
  const save = $api.useMutation("patch", "/api/v1/admin/leagues/{id}", {
    onSuccess: async () => {
      await refreshAfterWrite(queryClient);
      closeModal({ pathname: "/leagues/[id]", params: { id: league.id } });
    },
  });
  const set = <K extends keyof LeagueForm>(key: K, value: LeagueForm[K]) =>
    setForm((current) => ({ ...current, [key]: value }));
  const problems = checkLeague(form);
  const patch = patchBody(league, form);

  return (
    <Screen>
      <LeagueFields
        form={form}
        set={set}
        problems={tried ? problems : {}}
        creating={false}
        currentFormat={league.match_format}
      />
      {save.error ? <Text tone="danger">{describeError(save.error)}</Text> : null}
      <Button
        label="Save"
        block
        disabled={Object.keys(patch).length === 0}
        loading={save.isPending}
        onPress={() => {
          setTried(true);
          if (Object.keys(problems).length === 0) {
            save.mutate({ params: { path: { id: league.id } }, body: patch });
          }
        }}
      />
    </Screen>
  );
}
