import { useQueryClient } from "@tanstack/react-query";
import { router } from "expo-router";
import { useState } from "react";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { refreshAfterWrite } from "@/api/queryClient";
import { LeagueFields } from "@/features/admin/LeagueFields";
import {
  checkLeague,
  createBody,
  newLeagueForm,
  type LeagueForm,
} from "@/features/admin/leagueForm";
import { Button } from "@/ui/Button";
import { dayOf } from "@/ui/calendar";
import { Screen } from "@/ui/Screen";
import { Text } from "@/ui/Text";

/** A new league, created as a draft for the admin to check before publishing. */
export default function NewLeague() {
  const { $api } = useApi();
  const queryClient = useQueryClient();
  const [form, setForm] = useState(() => newLeagueForm(dayOf(new Date())));
  const [tried, setTried] = useState(false);
  const create = $api.useMutation("post", "/api/v1/admin/leagues", {
    onSuccess: async (league) => {
      await refreshAfterWrite(queryClient);
      router.replace({ pathname: "/leagues/[id]", params: { id: league.id } });
    },
  });
  const set = <K extends keyof LeagueForm>(key: K, value: LeagueForm[K]) =>
    setForm((current) => ({ ...current, [key]: value }));
  const problems = checkLeague(form);
  const ok = Object.keys(problems).length === 0;

  return (
    <Screen>
      <LeagueFields form={form} set={set} problems={tried ? problems : {}} creating />
      {create.error ? <Text tone="danger">{describeError(create.error)}</Text> : null}
      <Button
        label="Create draft"
        block
        loading={create.isPending}
        onPress={() => {
          setTried(true);
          if (ok) create.mutate({ body: createBody(form) });
        }}
      />
      <Text variant="caption" tone="textMuted" align="center">
        Members see the league once you publish it.
      </Text>
    </Screen>
  );
}
