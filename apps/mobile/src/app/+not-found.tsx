import { Link, Stack } from "expo-router";

import { EmptyState } from "@/ui/States";
import { Text } from "@/ui/Text";

/** Unknown paths, e.g. a stale shared link. */
export default function NotFound() {
  return (
    <>
      <Stack.Screen options={{ title: "Not found" }} />
      <EmptyState icon="help-circle-outline" title="This page doesn’t exist">
        <Link href="/">
          <Text tone="primaryText" weight="semibold">
            Go to the home screen
          </Text>
        </Link>
      </EmptyState>
    </>
  );
}
