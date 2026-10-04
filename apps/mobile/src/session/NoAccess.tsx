import { useQueryClient } from "@tanstack/react-query";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { useCommunity } from "@/tenant/TenantProvider";
import { Button } from "@/ui/Button";
import { EmptyState } from "@/ui/States";

import { useSession } from "./SessionProvider";

/**
 * Signed in, but the server refuses this community: the account was banned here, or (rarely)
 * signed in elsewhere and never joined. Joining is offered; the server decides.
 */
export function NoAccess({ error }: { error: unknown }) {
  const { $api } = useApi();
  const { signOut } = useSession();
  const community = useCommunity();
  const queryClient = useQueryClient();
  const join = $api.useMutation("post", "/api/v1/me/join", {
    onSuccess: () => queryClient.invalidateQueries(),
  });
  return (
    <EmptyState
      icon="lock-closed-outline"
      title={`You can’t open ${community.name}`}
      body={describeError(join.error ?? error)}
    >
      <Button
        label={`Join ${community.name}`}
        loading={join.isPending}
        onPress={() => join.mutate({})}
      />
      <Button label="Sign out" variant="ghost" onPress={() => void signOut()} />
    </EmptyState>
  );
}
