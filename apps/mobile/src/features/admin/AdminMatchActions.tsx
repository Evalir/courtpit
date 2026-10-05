import type { components } from "@racquetcollective/api-client";
import { useQueryClient } from "@tanstack/react-query";
import { router } from "expo-router";
import { useState } from "react";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { refreshAfterWrite } from "@/api/queryClient";
import type { MatchView } from "@/features/matches/match";
import { useSignedIn } from "@/session/SessionProvider";
import { space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
import { Card } from "@/ui/Card";
import { Segmented } from "@/ui/Segmented";
import { Text } from "@/ui/Text";

import { NoteConfirm } from "./NoteConfirm";
import { adminActions } from "./rulings";

type Side = components["schemas"]["Side"];
type Step = "replay" | "void" | "walkover" | "cancel" | null;

/** A club admin's decisions on a match: settle a dispute, award a walkover, call it off. */
export function AdminMatchActions({
  match,
  sides,
}: {
  match: MatchView;
  sides: Record<Side, string>;
}) {
  const session = useSignedIn();
  const { $api } = useApi();
  const queryClient = useQueryClient();
  const [step, setStep] = useState<Step>(null);
  const [winner, setWinner] = useState<Side>("a");
  const options = {
    onSuccess: async () => {
      setStep(null);
      await refreshAfterWrite(queryClient);
    },
  };
  const resolve = $api.useMutation("post", "/api/v1/admin/matches/{id}/resolve", options);
  const walkover = $api.useMutation("post", "/api/v1/admin/matches/{id}/walkover", options);
  const cancel = $api.useMutation("post", "/api/v1/matches/{id}/cancel", options);
  const actions = adminActions(match, { id: session.player_id, role: session.role });
  const path = { id: match.id };
  const error = resolve.error ?? walkover.error ?? cancel.error;

  if (!actions.resolve && !actions.walkover && !actions.cancel && !actions.blocked) return null;
  const back = () => setStep(null);
  return (
    <Card>
      <Text variant="overline" tone="textMuted">
        As club admin
      </Text>
      {actions.blocked ? <Text tone="textMuted">{actions.blocked}</Text> : null}
      {error ? <Text tone="danger">{describeError(error)}</Text> : null}
      {step === "replay" || step === "void" ? (
        <NoteConfirm
          prompt={
            step === "replay"
              ? "Order a replay? The reported score is cleared and the match is scheduled again."
              : "Void the match? It is cancelled and counts for nobody."
          }
          confirmLabel={step === "replay" ? "Order a replay" : "Void the match"}
          danger={step === "void"}
          loading={resolve.isPending}
          onConfirm={(note) =>
            resolve.mutate({ params: { path }, body: { resolution: step, note } })
          }
          onBack={back}
        />
      ) : step === "walkover" ? (
        <NoteConfirm
          prompt="Award the match without playing it (a no-show or a missed deadline)."
          confirmLabel={`Award to ${sides[winner]}`}
          loading={walkover.isPending}
          onConfirm={(note) =>
            walkover.mutate({ params: { path }, body: { winner_side: winner, note } })
          }
          onBack={back}
        >
          <Segmented
            options={[
              { value: "a" as const, label: sides.a },
              { value: "b" as const, label: sides.b },
            ]}
            value={winner}
            onChange={setWinner}
          />
        </NoteConfirm>
      ) : step === "cancel" ? (
        <NoteConfirm
          prompt="Cancel the match? It won’t be played or count for anyone."
          confirmLabel="Cancel the match"
          danger
          loading={cancel.isPending}
          onConfirm={(note) => cancel.mutate({ params: { path }, body: { note } })}
          onBack={back}
        />
      ) : (
        <View style={{ gap: space.sm }}>
          {actions.resolve ? (
            <>
              <Button
                label="Set the score"
                onPress={() =>
                  router.push({ pathname: "/admin/matches/[id]/resolve", params: { id: match.id } })
                }
              />
              <Button
                label="Order a replay"
                variant="secondary"
                onPress={() => setStep("replay")}
              />
              <Button label="Void the match" variant="ghost" onPress={() => setStep("void")} />
            </>
          ) : null}
          {actions.walkover ? (
            <Button
              label="Award a walkover"
              variant="secondary"
              onPress={() => setStep("walkover")}
            />
          ) : null}
          {actions.cancel ? (
            <Button label="Cancel the match" variant="ghost" onPress={() => setStep("cancel")} />
          ) : null}
        </View>
      )}
    </Card>
  );
}
