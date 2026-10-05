import type { components } from "@racquetcollective/api-client";
import { useQueryClient } from "@tanstack/react-query";
import * as Linking from "expo-linking";
import { useEffect, useState } from "react";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { refreshAfterWrite } from "@/api/queryClient";
import { pushPermission, registerForPush, type PushPermission } from "@/session/push";
import { Button } from "@/ui/Button";
import { Card } from "@/ui/Card";
import { Screen, Section } from "@/ui/Screen";
import { ErrorState, LoadingState } from "@/ui/States";
import { Text } from "@/ui/Text";
import { Toggle } from "@/ui/Toggle";

type Prefs = components["schemas"]["NotificationPrefs"];

const CATEGORIES: { key: keyof Prefs; label: string; hint: string }[] = [
  {
    key: "match_updates",
    label: "Match updates",
    hint: "Times proposed or accepted, scores reported or disputed, and admins’ decisions.",
  },
  {
    key: "league_updates",
    label: "League updates",
    hint: "Partner invitations and news about your leagues.",
  },
  { key: "reminders", label: "Reminders", hint: "The day before a scheduled match." },
];

/** What the player wants to hear about, and whether this device can tell them. */
export default function NotificationSettings() {
  const { $api } = useApi();
  const queryClient = useQueryClient();
  const prefs = $api.useQuery("get", "/api/v1/me/notifications");
  const save = $api.useMutation("put", "/api/v1/me/notifications", {
    onSuccess: () => refreshAfterWrite(queryClient),
  });

  if (prefs.isPending) return <LoadingState />;
  if (prefs.error) return <ErrorState error={prefs.error} onRetry={() => void prefs.refetch()} />;
  const current = save.variables?.body ?? prefs.data;

  return (
    <Screen>
      <DevicePush />
      <Section title="Tell me about">
        {CATEGORIES.map(({ key, label, hint }) => (
          <Toggle
            key={key}
            label={label}
            hint={hint}
            value={current[key]}
            onChange={(value) => save.mutate({ body: { ...current, [key]: value } })}
          />
        ))}
        {save.error ? <Text tone="danger">{describeError(save.error)}</Text> : null}
      </Section>
    </Screen>
  );
}

/** Whether this device shows pushes, with the way to change it. */
function DevicePush() {
  const { fetch } = useApi();
  const [permission, setPermission] = useState<PushPermission | null>(null);
  useEffect(() => {
    void pushPermission().then(setPermission, () => setPermission("unavailable"));
  }, []);
  if (permission === null) return null;
  return (
    <Card>
      {permission === "granted" ? (
        <Text>Notifications are on for this device.</Text>
      ) : permission === "undetermined" ? (
        <>
          <Text>Notifications are off for this device.</Text>
          <Button
            label="Turn on"
            size="sm"
            onPress={() => void registerForPush(fetch, true).then(setPermission)}
          />
        </>
      ) : permission === "denied" ? (
        <>
          <Text>Notifications are blocked for this app in your device’s settings.</Text>
          <Button label="Open settings" size="sm" onPress={() => void Linking.openSettings()} />
        </>
      ) : (
        <Text tone="textMuted">
          This device can’t show notifications. A score waiting for your answer is emailed to you
          instead.
        </Text>
      )}
    </Card>
  );
}
