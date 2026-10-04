import { router } from "expo-router";
import { useState } from "react";
import { KeyboardAvoidingView, Platform, ScrollView, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { CommunityMark } from "@/tenant/CommunityMark";
import { useCommunity } from "@/tenant/TenantProvider";
import { createStyles } from "@/theme/ThemeProvider";
import { maxContentWidth, radius, space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
import { Card } from "@/ui/Card";
import { Text } from "@/ui/Text";
import { TextField } from "@/ui/TextField";

const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

/** Step 1 of sign-in: the email address. A code is the only way in (spec §6). */
export default function SignIn() {
  const styles = useStyles();
  const insets = useSafeAreaInsets();
  const community = useCommunity();
  const { $api } = useApi();
  const [email, setEmail] = useState("");
  const [invalid, setInvalid] = useState(false);
  const request = $api.useMutation("post", "/api/v1/auth/otp/request", {
    onSuccess: (_data, variables) =>
      router.push({ pathname: "/verify", params: { email: variables.body.email } }),
  });

  const submit = () => {
    const address = email.trim();
    if (!EMAIL.test(address)) {
      setInvalid(true);
      return;
    }
    request.mutate({ body: { email: address } });
  };

  return (
    <KeyboardAvoidingView
      behavior={Platform.OS === "ios" ? "padding" : undefined}
      style={styles.screen}
    >
      <ScrollView contentContainerStyle={styles.scroll} keyboardShouldPersistTaps="handled">
        <View style={[styles.hero, { paddingTop: insets.top + space.xxxl }]}>
          <CommunityMark size={64} />
          <Text variant="overline" tone="onPrimary" style={[styles.dim, styles.welcome]}>
            Welcome to
          </Text>
          <Text variant="display" tone="onPrimary" accessibilityRole="header">
            {community.name}
          </Text>
          <Text tone="onPrimary" style={styles.dim}>
            Find a match, join the league, climb the rankings.
          </Text>
        </View>
        <View style={styles.body}>
          <Card style={styles.card}>
            <Text variant="heading">Sign in or join</Text>
            <TextField
              label="Email"
              value={email}
              onChangeText={(text) => {
                setEmail(text);
                setInvalid(false);
              }}
              onSubmitEditing={submit}
              placeholder="you@example.com"
              autoCapitalize="none"
              autoCorrect={false}
              autoComplete="email"
              inputMode="email"
              textContentType="emailAddress"
              returnKeyType="send"
              error={
                invalid
                  ? "Enter an email address like name@example.com."
                  : request.error
                    ? describeError(request.error)
                    : null
              }
            />
            <Button label="Email me a code" block loading={request.isPending} onPress={submit} />
          </Card>
          <Text variant="caption" tone="textMuted" align="center">
            We’ll email you a 6-digit code. New here? The same code creates your account.
          </Text>
        </View>
      </ScrollView>
    </KeyboardAvoidingView>
  );
}

const useStyles = createStyles(({ colors }) => ({
  screen: { flex: 1, backgroundColor: colors.background },
  scroll: { flexGrow: 1 },
  hero: {
    backgroundColor: colors.primary,
    paddingHorizontal: space.xl,
    paddingBottom: space.xxxl + space.xl,
    gap: space.sm,
  },
  dim: { opacity: 0.85 },
  welcome: { marginTop: space.md },
  body: {
    width: "100%",
    maxWidth: maxContentWidth,
    alignSelf: "center",
    paddingHorizontal: space.lg,
    marginTop: -space.xxl,
    gap: space.lg,
  },
  card: { gap: space.lg, padding: space.xl, borderRadius: radius.lg },
}));
