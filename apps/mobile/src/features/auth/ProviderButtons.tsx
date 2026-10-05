import { useMutation } from "@tanstack/react-query";
import * as AppleAuthentication from "expo-apple-authentication";
import { useEffect, useState } from "react";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { deviceLabel } from "@/session/device";
import {
  appleAvailable,
  appleToken,
  googleAvailable,
  googleToken,
  type ProviderToken,
} from "@/session/providers";
import { useSession } from "@/session/SessionProvider";
import { radius, space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
import { Text } from "@/ui/Text";

/** "Continue with Apple" (iOS) and "Continue with Google" when the build is set up for them. */
export function ProviderButtons() {
  const { fetch } = useApi();
  const { signIn } = useSession();
  const [apple, setApple] = useState(false);
  const google = googleAvailable();
  useEffect(() => {
    void appleAvailable().then(setApple, () => setApple(false));
  }, []);
  const login = useMutation({
    mutationFn: async (ask: () => Promise<ProviderToken | null>) => {
      const token = await ask();
      if (!token) return;
      const { data, error } = await fetch.POST("/api/v1/auth/oidc/{provider}", {
        params: { path: { provider: token.provider } },
        body: { id_token: token.id_token, nonce: token.nonce ?? null, device_label: deviceLabel },
      });
      if (error !== undefined) throw error;
      await signIn(data);
    },
  });

  if (!apple && !google) return null;
  return (
    <View style={{ gap: space.md }}>
      <Text variant="caption" tone="textMuted" align="center">
        or
      </Text>
      {apple ? (
        <AppleAuthentication.AppleAuthenticationButton
          buttonType={AppleAuthentication.AppleAuthenticationButtonType.CONTINUE}
          buttonStyle={AppleAuthentication.AppleAuthenticationButtonStyle.BLACK}
          cornerRadius={radius.md}
          style={{ height: 48 }}
          onPress={() => login.mutate(appleToken)}
        />
      ) : null}
      {google ? (
        <Button
          label="Continue with Google"
          icon="logo-google"
          variant="secondary"
          block
          loading={login.isPending}
          onPress={() => login.mutate(googleToken)}
        />
      ) : null}
      {login.error ? <Text tone="danger">{describeError(login.error)}</Text> : null}
    </View>
  );
}
