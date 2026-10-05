import type { components } from "@racquetcollective/api-client";

type PlayerRole = components["schemas"]["PlayerRole"];
type PlayerStatus = components["schemas"]["PlayerStatus"];

/** Admins and owners run the club. */
export function isAdmin(role: PlayerRole): boolean {
  return role === "admin" || role === "owner";
}

/**
 * The moderation the viewer may apply to a member, mirroring the server: never to oneself or a
 * deleted account, and only to someone of lower rank (owners may moderate admins).
 */
export function moderation(
  viewer: { id: string; role: PlayerRole },
  target: { id: string; role: PlayerRole; status: PlayerStatus },
): "ban" | "unban" | null {
  if (!isAdmin(viewer.role) || viewer.id === target.id || target.status === "deleted") {
    return null;
  }
  const outranks = target.role === "player" || (target.role === "admin" && viewer.role === "owner");
  if (!outranks) return null;
  return target.status === "banned" ? "unban" : "ban";
}
