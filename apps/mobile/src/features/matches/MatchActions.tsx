import { useQueryClient } from "@tanstack/react-query";
import { router } from "expo-router";
import { useState } from "react";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { refreshAfterWrite } from "@/api/queryClient";
import { formatDateTime } from "@/features/format";
import { createStyles } from "@/theme/ThemeProvider";
import { space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
import { Card } from "@/ui/Card";
import { Text } from "@/ui/Text";
import { TextField } from "@/ui/TextField";

import { matchActions, type MatchView } from "./match";

/**
 * What the viewer can do on a match: confirm or dispute a reported score, answer a proposed
 * time, propose one, report the score, or cancel a friendly. Renders nothing for bystanders.
 */
export function MatchActions({
  match,
  me,
  now,
  name,
}: {
  match: MatchView;
  me: string;
  now: Date;
  name: (id: string) => string;
}) {
  const styles = useStyles();
  const { $api } = useApi();
  const queryClient = useQueryClient();
  const options = { onSuccess: () => refreshAfterWrite(queryClient) };
  const confirm = $api.useMutation("post", "/api/v1/matches/{id}/confirm", options);
  const dispute = $api.useMutation("post", "/api/v1/matches/{id}/dispute", options);
  const accept = $api.useMutation(
    "post",
    "/api/v1/matches/{id}/proposals/{proposal_id}/accept",
    options,
  );
  const decline = $api.useMutation(
    "post",
    "/api/v1/matches/{id}/proposals/{proposal_id}/decline",
    options,
  );
  const cancel = $api.useMutation("post", "/api/v1/matches/{id}/cancel", options);
  const [disputing, setDisputing] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [note, setNote] = useState("");

  const actions = matchActions(match, me, now);
  const path = { id: match.id };
  const error =
    confirm.error ?? dispute.error ?? accept.error ?? decline.error ?? cancel.error ?? null;
  const errorText = error ? <Text tone="danger">{describeError(error)}</Text> : null;
  const when = (proposal: { proposed_time: string; location?: string | null }) =>
    `${formatDateTime(proposal.proposed_time)}${proposal.location ? ` at ${proposal.location}` : ""}`;

  if (actions.confirm) {
    return (
      <Card style={styles.action}>
        <Text variant="subheading">Is this score right?</Text>
        <Text variant="label" tone="textMuted">
          Confirming makes it final. If it’s wrong, dispute it and a club admin will decide.
        </Text>
        {disputing ? (
          <>
            <TextField
              label="What’s wrong with it?"
              value={note}
              onChangeText={setNote}
              placeholder="e.g. The second set was 6–4 to us"
              multiline
            />
            <View style={styles.buttons}>
              <Button
                label="Send dispute"
                variant="danger"
                loading={dispute.isPending}
                onPress={() =>
                  dispute.mutate({ params: { path }, body: { note: note.trim() || null } })
                }
              />
              <Button label="Back" variant="ghost" onPress={() => setDisputing(false)} />
            </View>
          </>
        ) : (
          <View style={styles.buttons}>
            <Button
              label="Confirm score"
              icon="checkmark"
              loading={confirm.isPending}
              onPress={() => confirm.mutate({ params: { path } })}
            />
            <Button label="Dispute" variant="secondary" onPress={() => setDisputing(true)} />
          </View>
        )}
        {errorText}
      </Card>
    );
  }

  if (!actions.propose && !actions.report) return null;

  // The likelier next step leads: arrange a proposed match, report a scheduled one.
  const reportButton = (primary: boolean) => (
    <Button
      key="report"
      label="Report score"
      icon="create-outline"
      variant={primary ? "primary" : "secondary"}
      onPress={() => router.push({ pathname: "/matches/[id]/report", params: { id: match.id } })}
    />
  );
  const proposeButton = (primary: boolean) => (
    <Button
      key="propose"
      label={actions.answer || actions.waiting ? "Suggest another time" : "Propose a time"}
      icon="calendar-outline"
      variant={primary ? "primary" : "secondary"}
      onPress={() => router.push({ pathname: "/matches/[id]/propose", params: { id: match.id } })}
    />
  );

  return (
    <Card style={styles.action}>
      {actions.answer ? (
        <>
          <Text variant="subheading">
            {name(actions.answer.proposed_by)} proposed {when(actions.answer)}
          </Text>
          <View style={styles.buttons}>
            <Button
              label="Accept"
              icon="checkmark"
              loading={accept.isPending}
              onPress={() =>
                accept.mutate({ params: { path: { ...path, proposal_id: actions.answer!.id } } })
              }
            />
            <Button
              label="Decline"
              variant="secondary"
              loading={decline.isPending}
              onPress={() =>
                decline.mutate({ params: { path: { ...path, proposal_id: actions.answer!.id } } })
              }
            />
          </View>
        </>
      ) : actions.waiting ? (
        <>
          <Text variant="subheading">You proposed {when(actions.waiting)}</Text>
          <Text variant="label" tone="textMuted">
            Waiting for the other side to answer.
          </Text>
        </>
      ) : (
        <Text variant="subheading">
          {match.status === "scheduled" ? "Played it?" : "When do you play?"}
        </Text>
      )}
      <View style={styles.buttons}>
        {match.status === "scheduled"
          ? [reportButton(true), proposeButton(false)]
          : [proposeButton(!actions.answer), reportButton(false)]}
      </View>
      {actions.cancel ? (
        cancelling ? (
          <View style={styles.cancel}>
            <TextField
              label="Reason (optional)"
              value={note}
              onChangeText={setNote}
              placeholder="e.g. Rained off"
            />
            <View style={styles.buttons}>
              <Button
                label="Cancel the match"
                variant="danger"
                loading={cancel.isPending}
                onPress={() =>
                  cancel.mutate({ params: { path }, body: { note: note.trim() || null } })
                }
              />
              <Button label="Keep it" variant="ghost" onPress={() => setCancelling(false)} />
            </View>
          </View>
        ) : (
          <Button
            label="Cancel match"
            variant="ghost"
            size="sm"
            onPress={() => setCancelling(true)}
          />
        )
      ) : null}
      {errorText}
    </Card>
  );
}

const useStyles = createStyles(({ colors }) => ({
  action: { borderColor: colors.primary, borderWidth: 2, gap: space.md },
  buttons: { flexDirection: "row", gap: space.sm, flexWrap: "wrap" },
  cancel: { gap: space.sm },
}));
