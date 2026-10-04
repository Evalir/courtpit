import { router } from "expo-router";
import { View } from "react-native";

import { disciplineLabel, formatDateTime } from "@/features/format";
import { sideName } from "@/features/players/names";
import { createStyles } from "@/theme/ThemeProvider";
import { space } from "@/theme/tokens";
import { Badge } from "@/ui/Badge";
import { Card } from "@/ui/Card";
import { Text } from "@/ui/Text";

import { actionFor, formatScore, sideOf, statusBadge, type MatchView } from "./match";

/** One match in a list: who, when and where, the score, and what the viewer should do. */
export function MatchCard({
  match,
  me,
  name,
  now,
}: {
  match: MatchView;
  me: string;
  name: (id: string) => string;
  now: Date;
}) {
  const styles = useStyles();
  // The viewer's side reads first, and the score from their point of view.
  const mine = sideOf(match, me) ?? "a";
  const [us, them] = mine === "a" ? [match.side_a, match.side_b] : [match.side_b, match.side_a];
  const title = `${sideName(us, name)} vs ${sideName(them, name)}`;
  const badge = statusBadge[match.status];
  const action = actionFor(match, me, now);
  const won = match.winner_side ? match.winner_side === mine : null;
  const where = [match.scheduled_at && formatDateTime(match.scheduled_at), match.location]
    .filter(Boolean)
    .join(" · ");

  return (
    <Card
      accessibilityLabel={`${title}, ${badge.label}`}
      onPress={() => router.push({ pathname: "/matches/[id]", params: { id: match.id } })}
    >
      <View style={styles.badges}>
        <Badge label={badge.label} tone={badge.tone} />
        {match.league_id ? <Badge label="League" tone="accent" /> : null}
        <Text variant="caption" tone="textMuted">
          {disciplineLabel[match.discipline]}
        </Text>
      </View>
      <Text variant="subheading">{title}</Text>
      {where ? (
        <Text variant="label" tone="textMuted">
          {where}
        </Text>
      ) : null}
      {match.score ? (
        <View style={styles.score}>
          <Text variant="subheading" style={styles.mono}>
            {formatScore(match.score, mine)}
          </Text>
          {won !== null && sideOf(match, me) ? (
            <Text variant="label" tone={won ? "success" : "textMuted"} weight="semibold">
              {won ? "Won" : "Lost"}
            </Text>
          ) : null}
        </View>
      ) : null}
      {action === "confirm" ? (
        <Text variant="label" tone="warning" weight="semibold">
          Confirm or dispute this score
          {match.confirm_deadline_at ? ` by ${formatDateTime(match.confirm_deadline_at)}` : ""}
        </Text>
      ) : action === "report" ? (
        <Text variant="label" tone="warning" weight="semibold">
          Played? Report the score
        </Text>
      ) : null}
    </Card>
  );
}

const useStyles = createStyles(() => ({
  badges: { flexDirection: "row", alignItems: "center", gap: space.sm, flexWrap: "wrap" },
  score: { flexDirection: "row", alignItems: "baseline", gap: space.md },
  mono: { fontVariant: ["tabular-nums"] },
}));
