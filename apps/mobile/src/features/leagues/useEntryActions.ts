import { useQueryClient } from "@tanstack/react-query";

import { useApi } from "@/api/client";
import { refreshAfterWrite } from "@/api/queryClient";

/** The league-entry writes, each refreshing what it changes (entries, Home, standings). */
export function useEntryActions() {
  const { $api } = useApi();
  const queryClient = useQueryClient();
  const options = { onSuccess: () => refreshAfterWrite(queryClient) };
  const register = $api.useMutation("post", "/api/v1/leagues/{id}/entries", options);
  const accept = $api.useMutation(
    "post",
    "/api/v1/leagues/{id}/entries/{entry_id}/accept",
    options,
  );
  const decline = $api.useMutation(
    "post",
    "/api/v1/leagues/{id}/entries/{entry_id}/decline",
    options,
  );
  const withdraw = $api.useMutation(
    "post",
    "/api/v1/leagues/{id}/entries/{entry_id}/withdraw",
    options,
  );
  const partner = $api.useMutation(
    "post",
    "/api/v1/leagues/{id}/entries/{entry_id}/partner",
    options,
  );
  const error =
    register.error ?? accept.error ?? decline.error ?? withdraw.error ?? partner.error ?? null;
  return { register, accept, decline, withdraw, partner, error };
}
