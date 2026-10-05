import { useQueryClient } from "@tanstack/react-query";
import { router } from "expo-router";
import { useState } from "react";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { refreshAfterWrite } from "@/api/queryClient";
import { formatDate } from "@/features/format";
import type { LeagueView } from "@/features/leagues/league";
import { useSignedIn } from "@/session/SessionProvider";
import { space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
import { Card } from "@/ui/Card";
import { Text } from "@/ui/Text";

import { leagueAdmin } from "./leagueAdmin";
import { isAdmin } from "./roles";

/** A league's admin controls: edit and publish a draft, or cancel the league. */
export function AdminLeagueActions({ league }: { league: LeagueView }) {
  const session = useSignedIn();
  const { $api } = useApi();
  const queryClient = useQueryClient();
  const [confirming, setConfirming] = useState<"publish" | "cancel" | null>(null);
  const options = {
    onSuccess: async () => {
      setConfirming(null);
      await refreshAfterWrite(queryClient);
    },
  };
  const publish = $api.useMutation("post", "/api/v1/admin/leagues/{id}/publish", options);
  const cancel = $api.useMutation("post", "/api/v1/admin/leagues/{id}/cancel", options);
  const can = leagueAdmin(league);
  const path = { params: { path: { id: league.id } } };
  const error = publish.error ?? cancel.error;

  if (!isAdmin(session.role) || (!can.edit && !can.publish && !can.cancel)) return null;
  return (
    <Card>
      <Text variant="overline" tone="textMuted">
        As club admin
      </Text>
      {league.status === "draft" ? (
        <Text variant="label" tone="textMuted">
          {league.published_at
            ? `Published; registration opens ${formatDate(league.registration_opens_at)}.`
            : "A draft: only admins can see it."}
        </Text>
      ) : null}
      {error ? <Text tone="danger">{describeError(error)}</Text> : null}
      {confirming === "publish" ? (
        <Confirm
          prompt={`Publish ${league.name}? Members see it now, and registration opens ${formatDate(league.registration_opens_at)}.`}
          label="Publish"
          loading={publish.isPending}
          onConfirm={() => publish.mutate(path)}
          onBack={() => setConfirming(null)}
        />
      ) : confirming === "cancel" ? (
        <Confirm
          prompt={`Cancel ${league.name}? Its unplayed matches are cancelled and it can’t be reopened.`}
          label="Cancel the league"
          danger
          loading={cancel.isPending}
          onConfirm={() => cancel.mutate(path)}
          onBack={() => setConfirming(null)}
        />
      ) : (
        <View style={{ gap: space.sm }}>
          {can.publish ? <Button label="Publish" onPress={() => setConfirming("publish")} /> : null}
          {can.edit ? (
            <Button
              label="Edit"
              variant="secondary"
              onPress={() =>
                router.push({ pathname: "/admin/leagues/[id]/edit", params: { id: league.id } })
              }
            />
          ) : null}
          {can.cancel ? (
            <Button
              label="Cancel the league"
              variant="ghost"
              onPress={() => setConfirming("cancel")}
            />
          ) : null}
        </View>
      )}
    </Card>
  );
}

function Confirm({
  prompt,
  label,
  danger = false,
  loading,
  onConfirm,
  onBack,
}: {
  prompt: string;
  label: string;
  danger?: boolean;
  loading: boolean;
  onConfirm: () => void;
  onBack: () => void;
}) {
  return (
    <View style={{ gap: space.sm }}>
      <Text variant="label">{prompt}</Text>
      <View style={{ flexDirection: "row", gap: space.sm }}>
        <Button
          label={label}
          variant={danger ? "danger" : "primary"}
          size="sm"
          loading={loading}
          onPress={onConfirm}
        />
        <Button label="Back" variant="ghost" size="sm" onPress={onBack} />
      </View>
    </View>
  );
}
