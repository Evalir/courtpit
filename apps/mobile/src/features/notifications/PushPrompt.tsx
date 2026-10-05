import * as SecureStore from "expo-secure-store";
import { useEffect, useState } from "react";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { pushPermission, registerForPush, type PushPermission } from "@/session/push";
import { space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
import { Card } from "@/ui/Card";
import { Text } from "@/ui/Text";

import { promptDue } from "./prompt";

const SNOOZED_KEY = "racquetcollective.pushPromptSnoozedAt";

/**
 * Offers push on Home, where its value is obvious, instead of asking at launch. Shown only while
 * the system would still ask, and not for a while after "Not now".
 */
export function PushPrompt() {
  const { fetch } = useApi();
  const [permission, setPermission] = useState<PushPermission | null>(null);
  const [due, setDue] = useState(false);
  const [asking, setAsking] = useState(false);

  useEffect(() => {
    // Only a device that can still ask reads the snooze (SecureStore is native-only).
    const check = async () => {
      const current = await pushPermission();
      setPermission(current);
      if (current !== "undetermined") return;
      const snoozed = await SecureStore.getItemAsync(SNOOZED_KEY).catch(() => null);
      setDue(promptDue(snoozed, new Date()));
    };
    void check().catch(() => setPermission("unavailable"));
  }, []);

  if (permission !== "undetermined" || !due) return null;
  return (
    <Card>
      <Text variant="subheading">Know when it’s your move</Text>
      <Text variant="label" tone="textMuted">
        Get a notification when someone proposes a time, reports a score or invites you to partner
        them.
      </Text>
      <View style={{ flexDirection: "row", gap: space.sm }}>
        <Button
          label="Turn on"
          size="sm"
          loading={asking}
          onPress={() => {
            setAsking(true);
            void registerForPush(fetch, true)
              .then(setPermission, () => setPermission("unavailable"))
              .finally(() => setAsking(false));
          }}
        />
        <Button
          label="Not now"
          size="sm"
          variant="ghost"
          onPress={() => {
            setDue(false);
            void SecureStore.setItemAsync(SNOOZED_KEY, new Date().toISOString()).catch(
              () => undefined,
            );
          }}
        />
      </View>
    </Card>
  );
}
