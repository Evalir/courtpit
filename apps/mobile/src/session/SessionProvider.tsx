import type { components } from "@courtpit/api-client";
import { useQueryClient } from "@tanstack/react-query";
import { createContext, use, useEffect, useState, type ReactNode } from "react";

import { useApi } from "@/api/client";
import { errorCode } from "@/api/errors";
import { onSessionExpired, queryPath } from "@/api/queryClient";

import { sessionToken } from "./token";

type SessionInfo = components["schemas"]["SessionInfo"];
type AuthSession = components["schemas"]["AuthSession"];

/** Who is using the app, as far as the client knows. */
export type Session =
  | { status: "loading" }
  | { status: "signed-out" }
  | { status: "signed-in"; info: SessionInfo }
  /** Signed in, but not a member here or banned (`GET /auth/session` → 403). */
  | { status: "no-access"; error: unknown }
  /** Couldn't ask the server (offline, outage); the stored session is kept. */
  | { status: "error"; error: unknown; retry: () => void };

interface SessionContextValue {
  session: Session;
  /** Stores a new session from sign-in and loads who it belongs to. */
  signIn(auth: AuthSession): Promise<void>;
  /** Ends the session on the server (best effort) and forgets it locally. */
  signOut(): Promise<void>;
}

const SessionContext = createContext<SessionContextValue | null>(null);

// boot: reading the stored token (native); check: asking the server; out: no session.
type Phase = "boot" | "check" | "out";

/**
 * Owns the session lifecycle. Native keeps a bearer token in secure storage; web relies on the
 * httpOnly cookie, so "is there a session" is only answerable by asking `GET /auth/session`.
 * Any `unauthorized` answer anywhere ends the session (see `createQueryClient`).
 */
export function SessionProvider({ children }: { children: ReactNode }) {
  const { $api, fetch } = useApi();
  const queryClient = useQueryClient();
  const [phase, setPhase] = useState<Phase>(sessionToken.stored ? "boot" : "check");

  useEffect(() => {
    if (!sessionToken.stored) return;
    sessionToken.load().then(
      (token) => setPhase(token ? "check" : "out"),
      () => setPhase("out"),
    );
  }, []);

  // Everything cached belongs to the previous identity; keep only the community itself.
  const forgetUserData = () =>
    queryClient.removeQueries({ predicate: (query) => queryPath(query) !== "/api/v1/tenant" });

  const endLocally = async () => {
    await sessionToken.clear().catch(() => undefined);
    forgetUserData();
    setPhase("out");
  };

  useEffect(() => onSessionExpired(() => void endLocally()));

  const check = $api.useQuery("get", "/api/v1/auth/session", undefined, {
    enabled: phase === "check",
    staleTime: Infinity,
  });

  const session = ((): Session => {
    if (phase === "boot") return { status: "loading" };
    if (phase === "out") return { status: "signed-out" };
    if (check.data) return { status: "signed-in", info: check.data };
    if (check.error) {
      const code = errorCode(check.error);
      if (code === "unauthorized") return { status: "signed-out" };
      if (code === "forbidden") return { status: "no-access", error: check.error };
      return { status: "error", error: check.error, retry: () => void check.refetch() };
    }
    return { status: "loading" };
  })();

  const value: SessionContextValue = {
    session,
    async signIn(auth) {
      if (auth.token) await sessionToken.save(auth.token);
      forgetUserData();
      setPhase("check");
    },
    async signOut() {
      await fetch.POST("/api/v1/auth/logout").catch(() => undefined);
      await endLocally();
    },
  };

  return <SessionContext value={value}>{children}</SessionContext>;
}

/** The session and the sign-in / sign-out actions. */
export function useSession(): SessionContextValue {
  const value = use(SessionContext);
  if (!value) throw new Error("useSession outside SessionProvider");
  return value;
}

/** The signed-in player's session; only for screens behind the signed-in guard. */
export function useSignedIn(): SessionInfo {
  const { session } = useSession();
  if (session.status !== "signed-in") throw new Error("useSignedIn while not signed in");
  return session.info;
}
