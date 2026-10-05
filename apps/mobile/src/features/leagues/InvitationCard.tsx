import { View } from "react-native";

import { describeError } from "@/api/errors";
import { space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
import { Card } from "@/ui/Card";
import { Text } from "@/ui/Text";

import type { EntryView } from "./entries";
import { useEntryActions } from "./useEntryActions";

/** "Ana Silva invited you to play Autumn doubles together": accept or decline. */
export function InvitationCard({
  entry,
  from,
  league,
}: {
  entry: EntryView;
  from: string;
  /** The league's name, when the card is shown outside the league's page. */
  league?: string;
}) {
  const actions = useEntryActions();
  const path = { params: { path: { id: entry.league_id, entry_id: entry.id } } };
  return (
    <Card>
      <Text variant="subheading">
        {league
          ? `${from} invited you to play ${league} together`
          : `${from} invited you to play together`}
      </Text>
      <Text variant="label" tone="textMuted">
        Accepting enters you both; any entry of your own in this league is withdrawn.
      </Text>
      {actions.error ? <Text tone="danger">{describeError(actions.error)}</Text> : null}
      <View style={{ flexDirection: "row", gap: space.sm }}>
        <Button
          label="Accept"
          size="sm"
          loading={actions.accept.isPending}
          onPress={() => actions.accept.mutate(path)}
        />
        <Button
          label="Decline"
          size="sm"
          variant="ghost"
          loading={actions.decline.isPending}
          onPress={() => actions.decline.mutate(path)}
        />
      </View>
    </Card>
  );
}
