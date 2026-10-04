import Constants from "expo-constants";
import { router, useLocalSearchParams } from "expo-router";
import { useState } from "react";
import { Platform, View } from "react-native";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { useSession } from "@/session/SessionProvider";
import { createStyles } from "@/theme/ThemeProvider";
import { space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
import { Screen } from "@/ui/Screen";
import { Text } from "@/ui/Text";
import { TextField } from "@/ui/TextField";

const CODE_LENGTH = 6;

/** Shown in the account's session list ("Ana's iPhone"). */
const deviceLabel =
  Constants.deviceName ?? (Platform.OS === "web" ? "Web browser" : `${Platform.OS} app`);

/** Step 2 of sign-in: the emailed code. Submits by itself once six digits are in. */
export default function Verify() {
  const styles = useStyles();
  const { email = "" } = useLocalSearchParams<{ email: string }>();
  const { $api } = useApi();
  const { signIn } = useSession();
  const [code, setCode] = useState("");
  const verify = $api.useMutation("post", "/api/v1/auth/otp/verify", {
    onSuccess: (auth) => signIn(auth),
  });
  const resend = $api.useMutation("post", "/api/v1/auth/otp/request");

  const submit = (value = code) => {
    if (value.length === CODE_LENGTH && !verify.isPending) {
      verify.mutate({ body: { email, code: value, device_label: deviceLabel } });
    }
  };

  return (
    <Screen>
      <View style={styles.intro}>
        <Text variant="title" accessibilityRole="header">
          Check your email
        </Text>
        <Text tone="textMuted">
          We sent a {CODE_LENGTH}-digit code to <Text weight="semibold">{email}</Text>. It works for
          10 minutes.
        </Text>
      </View>
      <TextField
        label="Code"
        value={code}
        onChangeText={(text) => {
          const digits = text.replace(/\D/g, "").slice(0, CODE_LENGTH);
          setCode(digits);
          verify.reset();
          submit(digits);
        }}
        onSubmitEditing={() => submit()}
        autoFocus
        inputMode="numeric"
        autoComplete="one-time-code"
        textContentType="oneTimeCode"
        maxLength={CODE_LENGTH}
        style={styles.code}
        error={verify.error ? describeError(verify.error) : null}
        hint={resend.isSuccess ? "A new code is on its way." : undefined}
      />
      <Button
        label="Sign in"
        block
        loading={verify.isPending || verify.isSuccess}
        disabled={code.length !== CODE_LENGTH}
        onPress={() => submit()}
      />
      <View style={styles.links}>
        <Button
          label="Send a new code"
          variant="ghost"
          size="sm"
          loading={resend.isPending}
          onPress={() => resend.mutate({ body: { email } })}
        />
        <Button
          label="Use another email"
          variant="ghost"
          size="sm"
          onPress={() => (router.canGoBack() ? router.back() : router.replace("/sign-in"))}
        />
      </View>
      {resend.error ? (
        <Text variant="caption" tone="danger">
          {describeError(resend.error)}
        </Text>
      ) : null}
    </Screen>
  );
}

const useStyles = createStyles(() => ({
  intro: { gap: space.sm },
  code: { fontSize: 24, letterSpacing: 8, textAlign: "center" },
  links: { flexDirection: "row", justifyContent: "space-between" },
}));
