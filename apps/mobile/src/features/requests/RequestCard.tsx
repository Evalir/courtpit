import { useQueryClient } from "@tanstack/react-query";
import { router } from "expo-router";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { refreshAfterWrite } from "@/api/queryClient";
import { disciplineLabel } from "@/features/format";
import { nameLookup } from "@/features/players/names";
import { space } from "@/theme/tokens";
import { Badge } from "@/ui/Badge";
import { Button } from "@/ui/Button";
import { Card } from "@/ui/Card";
import { Text } from "@/ui/Text";

import {
  fitsLevel,
  levelLabel,
  requestRole,
  spotsLabel,
  windowLabel,
  type MatchRequestView,
} from "./request";

/** An open call for a friendly: when, where, level, who is in, and join/leave/cancel. */
export function RequestCard({
  request,
  me,
  myUtr,
}: {
  request: MatchRequestView;
  me: string;
  myUtr: number | null | undefined;
}) {
  const { $api } = useApi();
  const queryClient = useQueryClient();
  const path = { params: { path: { id: request.id } } };
  const done = {
    onSuccess: async (updated: MatchRequestView) => {
      await refreshAfterWrite(queryClient);
      // The last slot fills the request and creates the match: go there.
      if (updated.match_id) {
        router.push({ pathname: "/matches/[id]", params: { id: updated.match_id } });
      }
    },
  };
  const join = $api.useMutation("post", "/api/v1/match-requests/{id}/join", done);
  const leave = $api.useMutation("post", "/api/v1/match-requests/{id}/leave", done);
  const cancel = $api.useMutation("post", "/api/v1/match-requests/{id}/cancel", done);
  const role = requestRole(request, me);
  const name = nameLookup(request.names, me);
  const fits = fitsLevel(myUtr, request.utr_min, request.utr_max);
  const error = join.error ?? leave.error ?? cancel.error;

  return (
    <Card>
      <View style={{ flexDirection: "row", gap: space.sm, alignItems: "center", flexWrap: "wrap" }}>
        <Badge label={disciplineLabel[request.discipline]} tone="primary" />
        <Badge label={spotsLabel(request.slots_open)} tone="accent" />
        {role === "creator" ? <Badge label="Yours" /> : null}
        {role === "joined" ? <Badge label="You’re in" tone="success" /> : null}
      </View>
      <Text variant="subheading">
        {windowLabel(request.time_window_start, request.time_window_end)}
      </Text>
      <Text variant="label" tone="textMuted">
        {[request.location, levelLabel(request.utr_min, request.utr_max)]
          .filter(Boolean)
          .join(" · ")}
      </Text>
      <Text variant="label">{request.players.map(name).join(", ")}</Text>
      {error ? <Text tone="danger">{describeError(error)}</Text> : null}
      <View style={{ flexDirection: "row", gap: space.sm }}>
        {role === "open" ? (
          <Button
            label={fits ? "Join" : "Outside your level"}
            size="sm"
            disabled={!fits}
            loading={join.isPending}
            onPress={() => join.mutate(path)}
          />
        ) : role === "joined" ? (
          <Button
            label="Leave"
            size="sm"
            variant="secondary"
            loading={leave.isPending}
            onPress={() => leave.mutate(path)}
          />
        ) : (
          <Button
            label="Cancel request"
            size="sm"
            variant="ghost"
            loading={cancel.isPending}
            onPress={() => cancel.mutate(path)}
          />
        )}
      </View>
    </Card>
  );
}
