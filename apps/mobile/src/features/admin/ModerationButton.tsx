import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { refreshAfterWrite } from "@/api/queryClient";
import { space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
import { Text } from "@/ui/Text";

/** Ban (after a confirmation) or lift a ban, for admins. */
export function ModerationButton({
  player,
  action,
}: {
  player: { id: string; display_name: string };
  action: "ban" | "unban";
}) {
  const { $api } = useApi();
  const queryClient = useQueryClient();
  const [confirming, setConfirming] = useState(false);
  const options = {
    onSuccess: async () => {
      setConfirming(false);
      await refreshAfterWrite(queryClient);
    },
  };
  const ban = $api.useMutation("post", "/api/v1/admin/players/{id}/ban", options);
  const unban = $api.useMutation("post", "/api/v1/admin/players/{id}/unban", options);
  const path = { params: { path: { id: player.id } } };
  const error = ban.error ?? unban.error;
  const errorText = error ? <Text tone="danger">{describeError(error)}</Text> : null;

  if (action === "unban") {
    return (
      <>
        <Button
          label="Lift the ban"
          variant="secondary"
          size="sm"
          loading={unban.isPending}
          onPress={() => unban.mutate(path)}
        />
        {errorText}
      </>
    );
  }
  if (!confirming) {
    return (
      <Button
        label="Ban from the club"
        variant="ghost"
        size="sm"
        onPress={() => setConfirming(true)}
      />
    );
  }
  return (
    <View style={{ gap: space.sm, alignSelf: "stretch" }}>
      <Text variant="label">
        Ban {player.display_name}? They lose access to the club straight away; their matches stay.
      </Text>
      {errorText}
      <View style={{ flexDirection: "row", gap: space.sm }}>
        <Button
          label="Ban"
          variant="danger"
          size="sm"
          loading={ban.isPending}
          onPress={() => ban.mutate(path)}
        />
        <Button label="Keep" variant="ghost" size="sm" onPress={() => setConfirming(false)} />
      </View>
    </View>
  );
}
