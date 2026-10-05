import { useState } from "react";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { deletionConfirmed } from "@/features/profile/account";
import { useSession } from "@/session/SessionProvider";
import { space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
import { Card } from "@/ui/Card";
import { Screen } from "@/ui/Screen";
import { ErrorState, LoadingState } from "@/ui/States";
import { Text } from "@/ui/Text";
import { TextField } from "@/ui/TextField";

const CONSEQUENCES = [
  "Your name, rating, contact details and gear are erased in every club you belong to.",
  "Matches you played stay in their leagues and rankings as “Deleted player”.",
  "Your email, password and linked sign-ins are removed and every device is signed out.",
  "This can’t be undone. Download your data from your profile first if you want a copy.",
];

/** Delete the account, after typing its email. */
export default function DeleteAccount() {
  const { $api } = useApi();
  const { signOut } = useSession();
  const me = $api.useQuery("get", "/api/v1/me");
  const [typed, setTyped] = useState("");
  const remove = $api.useMutation("delete", "/api/v1/me", {
    // The server ended every session; signing out clears this device.
    onSuccess: () => signOut(),
  });

  if (me.isPending) return <LoadingState />;
  if (me.error) return <ErrorState error={me.error} onRetry={() => void me.refetch()} />;
  const email = me.data.account.email;

  return (
    <Screen>
      <Card>
        <View style={{ gap: space.sm }}>
          {CONSEQUENCES.map((line) => (
            <Text key={line} variant="label">
              • {line}
            </Text>
          ))}
        </View>
      </Card>
      <TextField
        label={`Type ${email} to confirm`}
        value={typed}
        onChangeText={setTyped}
        autoCapitalize="none"
        autoCorrect={false}
        keyboardType="email-address"
      />
      {remove.error ? <Text tone="danger">{describeError(remove.error)}</Text> : null}
      <Button
        label="Delete my account"
        variant="danger"
        block
        disabled={!deletionConfirmed(typed, email)}
        loading={remove.isPending}
        onPress={() => remove.mutate({})}
      />
    </Screen>
  );
}
