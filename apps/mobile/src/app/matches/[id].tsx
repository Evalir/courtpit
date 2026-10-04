import type { components } from "@courtpit/api-client";
import { useQueryClient } from "@tanstack/react-query";
import { Link, useLocalSearchParams } from "expo-router";
import { useState } from "react";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { refreshAfterWrite } from "@/api/queryClient";
import { disciplineLabel, formatDateTime } from "@/features/format";
import { actionFor, sideOf, statusBadge, type MatchView } from "@/features/matches/match";
import { playerNames, sideName } from "@/features/players/names";
import { useSignedIn } from "@/session/SessionProvider";
import { createStyles } from "@/theme/ThemeProvider";
import { radius, space } from "@/theme/tokens";
import { Badge } from "@/ui/Badge";
import { Button } from "@/ui/Button";
import { Card } from "@/ui/Card";
import { Icon } from "@/ui/Icon";
import { Screen, Section } from "@/ui/Screen";
import { ErrorState, LoadingState } from "@/ui/States";
import { Text } from "@/ui/Text";
import { TextField } from "@/ui/TextField";

type Proposal = components["schemas"]["Proposal"];
type Side = components["schemas"]["Side"];

const proposalStatus: Record<Proposal["status"], string> = {
  open: "Waiting for an answer",
  accepted: "Accepted",
  declined: "Declined",
  superseded: "Replaced by a newer proposal",
};

/** A match: the scoreboard, where things stand, and the viewer's next step. */
export default function Match() {
  const styles = useStyles();
  const { id } = useLocalSearchParams<{ id: string }>();
  const me = useSignedIn().player_id;
  const { $api } = useApi();
  const match = $api.useQuery("get", "/api/v1/matches/{id}", { params: { path: { id } } });
  const leagueId = match.data?.league_id ?? "";
  const league = $api.useQuery(
    "get",
    "/api/v1/leagues/{id}",
    { params: { path: { id: leagueId } } },
    { enabled: leagueId !== "" },
  );
  const data = match.data;

  if (match.isPending) return <LoadingState />;
  if (match.error || !data)
    return <ErrorState error={match.error} onRetry={() => void match.refetch()} />;
  // Only the match's own players propose, report and dispute, so its sides name everyone.
  const name = playerNames([...data.side_a_names, ...data.side_b_names], me);
  const badge = statusBadge[data.status];
  const now = new Date(match.dataUpdatedAt);
  const when = [data.scheduled_at && formatDateTime(data.scheduled_at), data.location]
    .filter(Boolean)
    .join(" · ");

  return (
    <Screen refreshing={match.isRefetching} onRefresh={() => void match.refetch()}>
      <View style={styles.badges}>
        <Badge label={badge.label} tone={badge.tone} />
        <Text variant="caption" tone="textMuted">
          {disciplineLabel[data.discipline]}
          {data.round ? ` · Round ${data.round}` : ""}
        </Text>
      </View>
      {league.data ? (
        <Link href={{ pathname: "/leagues/[id]", params: { id: league.data.id } }}>
          <Text variant="label" tone="primaryText" weight="semibold">
            {league.data.name} ›
          </Text>
        </Link>
      ) : null}
      <Scoreboard match={data} name={name} />
      {when ? (
        <View style={styles.inline}>
          <Icon name="calendar-outline" tone="textMuted" />
          <Text tone="textMuted">{when}</Text>
        </View>
      ) : null}
      <StatusNotes match={data} name={name} />
      <NextStep match={data} me={me} now={now} name={name} />
      {data.proposals && data.proposals.length > 0 ? (
        <Section title="Scheduling">
          {data.proposals.map((proposal) => (
            <Card key={proposal.id}>
              <Text variant="label" weight="semibold">
                {formatDateTime(proposal.proposed_time)}
                {proposal.location ? ` · ${proposal.location}` : ""}
              </Text>
              <Text variant="caption" tone="textMuted">
                Proposed by {name(proposal.proposed_by)} · {proposalStatus[proposal.status]}
              </Text>
            </Card>
          ))}
        </Section>
      ) : null}
    </Screen>
  );
}

/** Both sides with their set scores; the winning side in bold with a check. */
function Scoreboard({ match, name }: { match: MatchView; name: (id: string) => string }) {
  const styles = useStyles();
  const sides: [Side, string[]][] = [
    ["a", match.side_a],
    ["b", match.side_b],
  ];
  return (
    <View style={styles.board}>
      {sides.map(([side, players], index) => {
        const won = match.winner_side === side;
        return (
          <View key={side} style={[styles.boardRow, index > 0 && styles.boardDivider]}>
            <View style={styles.boardNames}>
              <Text variant="subheading" weight={won ? "bold" : "regular"}>
                {sideName(players, name)}
              </Text>
              {won ? <Icon name="checkmark-circle" tone="success" size={18} /> : null}
            </View>
            {match.score?.sets.map((set, setIndex) => {
              const games = side === "a" ? set.a : set.b;
              const other = side === "a" ? set.b : set.a;
              return (
                <Text
                  key={setIndex}
                  variant="heading"
                  weight={games > other ? "bold" : "regular"}
                  tone={games > other ? "text" : "textMuted"}
                  style={styles.set}
                >
                  {games}
                </Text>
              );
            })}
          </View>
        );
      })}
    </View>
  );
}

/** Who reported, the confirmation deadline, a dispute or a ruling. */
function StatusNotes({ match, name }: { match: MatchView; name: (id: string) => string }) {
  const lines: string[] = [];
  if (match.reported_by && match.reported_at) {
    lines.push(`Reported by ${name(match.reported_by)} on ${formatDateTime(match.reported_at)}.`);
  }
  if (match.status === "reported" && match.confirm_deadline_at) {
    lines.push(
      `Confirms automatically on ${formatDateTime(match.confirm_deadline_at)} unless disputed.`,
    );
  }
  if (match.disputed_by) {
    lines.push(
      `Disputed by ${name(match.disputed_by)}${match.dispute_note ? `: “${match.dispute_note}”` : ""}. A club admin will decide.`,
    );
  }
  if (match.resolution_note) lines.push(`Admin ruling: ${match.resolution_note}`);
  if (lines.length === 0) return null;
  return (
    <Card>
      {lines.map((line) => (
        <Text key={line} variant="label" tone="textMuted">
          {line}
        </Text>
      ))}
    </Card>
  );
}

/** The viewer's actions: confirm or dispute a score, answer a proposal. */
function NextStep({
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
  const [disputing, setDisputing] = useState(false);
  const [note, setNote] = useState("");

  const mine = sideOf(match, me);
  const path = { id: match.id };
  const open = match.proposals?.find((proposal) => proposal.status === "open");
  const error = confirm.error ?? dispute.error ?? accept.error ?? decline.error;

  if (actionFor(match, me, now) === "confirm") {
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
        {error ? <Text tone="danger">{describeError(error)}</Text> : null}
      </Card>
    );
  }

  if (open && mine) {
    const theirs = sideOf(match, open.proposed_by) !== mine;
    const when = `${formatDateTime(open.proposed_time)}${open.location ? ` at ${open.location}` : ""}`;
    return (
      <Card style={styles.action}>
        <Text variant="subheading">
          {theirs ? `${name(open.proposed_by)} proposed ${when}` : `You proposed ${when}`}
        </Text>
        {theirs ? (
          <View style={styles.buttons}>
            <Button
              label="Accept"
              icon="checkmark"
              loading={accept.isPending}
              onPress={() => accept.mutate({ params: { path: { ...path, proposal_id: open.id } } })}
            />
            <Button
              label="Decline"
              variant="secondary"
              loading={decline.isPending}
              onPress={() =>
                decline.mutate({ params: { path: { ...path, proposal_id: open.id } } })
              }
            />
          </View>
        ) : (
          <Text variant="label" tone="textMuted">
            Waiting for the other side to answer.
          </Text>
        )}
        {error ? <Text tone="danger">{describeError(error)}</Text> : null}
      </Card>
    );
  }
  return null;
}

const useStyles = createStyles(({ colors }) => ({
  badges: { flexDirection: "row", alignItems: "center", gap: space.sm },
  inline: { flexDirection: "row", alignItems: "center", gap: space.sm },
  board: {
    backgroundColor: colors.surface,
    borderRadius: radius.lg,
    borderWidth: 1,
    borderColor: colors.border,
  },
  boardRow: {
    flexDirection: "row",
    alignItems: "center",
    paddingHorizontal: space.lg,
    paddingVertical: space.md,
    gap: space.md,
  },
  boardDivider: { borderTopWidth: 1, borderTopColor: colors.border },
  boardNames: { flex: 1, flexDirection: "row", alignItems: "center", gap: space.xs },
  set: { width: 28, textAlign: "center", fontVariant: ["tabular-nums"] },
  action: { borderColor: colors.primary, borderWidth: 2, gap: space.md },
  buttons: { flexDirection: "row", gap: space.sm, flexWrap: "wrap" },
}));
