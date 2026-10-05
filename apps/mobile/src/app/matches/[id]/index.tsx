import type { components } from "@courtpit/api-client";
import { Link, useLocalSearchParams } from "expo-router";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { disciplineLabel, formatDateTime } from "@/features/format";
import {
  outcomeNote,
  proposalLabel,
  sideOf,
  statusBadge,
  type MatchView,
} from "@/features/matches/match";
import { MatchActions } from "@/features/matches/MatchActions";
import { nameLookup, sideName } from "@/features/players/names";
import { useSignedIn } from "@/session/SessionProvider";
import { createStyles } from "@/theme/ThemeProvider";
import { radius, space } from "@/theme/tokens";
import { Badge } from "@/ui/Badge";
import { Card } from "@/ui/Card";
import { Icon } from "@/ui/Icon";
import { Screen, Section } from "@/ui/Screen";
import { ErrorState, LoadingState } from "@/ui/States";
import { Text } from "@/ui/Text";

type Side = components["schemas"]["Side"];

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
  const name = nameLookup(data?.names ?? [], me);

  if (match.isPending) return <LoadingState />;
  if (match.error || !data)
    return <ErrorState error={match.error} onRetry={() => void match.refetch()} />;
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
      <Scoreboard match={data} me={me} name={name} />
      {when ? (
        <View style={styles.inline}>
          <Icon name="calendar-outline" tone="textMuted" />
          <Text tone="textMuted">{when}</Text>
        </View>
      ) : null}
      <StatusNotes match={data} name={name} />
      <MatchActions match={data} me={me} now={now} name={name} />
      {data.proposals && data.proposals.length > 0 ? (
        <Section title="Scheduling">
          {data.proposals.map((proposal) => (
            <Card key={proposal.id}>
              <Text variant="label" weight="semibold">
                {formatDateTime(proposal.proposed_time)}
                {proposal.location ? ` · ${proposal.location}` : ""}
              </Text>
              <Text variant="caption" tone="textMuted">
                Proposed by {name(proposal.proposed_by)} · {proposalLabel(proposal, data)}
              </Text>
            </Card>
          ))}
        </Section>
      ) : null}
    </Screen>
  );
}

/** Both sides with their set scores; the winning side in bold with a check. */
function Scoreboard({
  match,
  me,
  name,
}: {
  match: MatchView;
  me: string;
  name: (id: string) => string;
}) {
  const styles = useStyles();
  const sides: [Side, string[]][] = [
    ["a", match.side_a],
    ["b", match.side_b],
  ];
  // The viewer's side on top, as on their match cards.
  if (sideOf(match, me) === "b") sides.reverse();
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

/** Who reported, the confirmation deadline, a dispute, a cancellation or a ruling. */
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
  const outcome = outcomeNote(match, name);
  if (outcome) lines.push(outcome);
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
}));
