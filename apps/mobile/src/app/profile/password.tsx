import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { refreshAfterWrite } from "@/api/queryClient";
import { passwordProblem } from "@/features/profile/account";
import { closeModal } from "@/features/navigation";
import { Button } from "@/ui/Button";
import { Screen } from "@/ui/Screen";
import { Text } from "@/ui/Text";
import { TextField } from "@/ui/TextField";

/** Set or change the account's password (signing in by email code keeps working). */
export default function Password() {
  const { $api } = useApi();
  const [password, setPassword] = useState("");
  const [again, setAgain] = useState("");
  const [tried, setTried] = useState(false);
  const queryClient = useQueryClient();
  const save = $api.useMutation("put", "/api/v1/auth/password", {
    onSuccess: async () => {
      await refreshAfterWrite(queryClient);
      closeModal("/profile");
    },
  });
  const problem = passwordProblem(password, again);

  return (
    <Screen>
      <Text tone="textMuted">
        A password lets you sign in without waiting for an email. Email codes keep working.
      </Text>
      <TextField
        label="New password"
        value={password}
        onChangeText={setPassword}
        secureTextEntry
        autoComplete="new-password"
        hint="At least 10 characters."
        error={tried ? problem?.password : null}
      />
      <TextField
        label="Repeat it"
        value={again}
        onChangeText={setAgain}
        secureTextEntry
        autoComplete="new-password"
        error={tried ? problem?.again : null}
      />
      {save.error ? <Text tone="danger">{describeError(save.error)}</Text> : null}
      <Button
        label="Save password"
        block
        loading={save.isPending}
        onPress={() => {
          setTried(true);
          if (!problem) save.mutate({ body: { password } });
        }}
      />
    </Screen>
  );
}
