import type { components } from "@courtpit/api-client";

import { formatUtr } from "@/features/format";

export type MatchRequestView = components["schemas"]["MatchRequestView"];

/** The viewer's relation to a request. */
export type RequestRole = "creator" | "joined" | "open";

export function requestRole(request: MatchRequestView, me: string): RequestRole {
  if (request.created_by === me) return "creator";
  return request.players.includes(me) ? "joined" : "open";
}

/** "Any level", "UTR 4.00–6.00", "UTR 4.00 and up", "Up to UTR 6.00". */
export function levelLabel(min?: number | null, max?: number | null): string {
  if (min == null && max == null) return "Any level";
  if (min != null && max != null) return `UTR ${formatUtr(min)}–${formatUtr(max)}`;
  if (min != null) return `UTR ${formatUtr(min)} and up`;
  return `Up to UTR ${formatUtr(max)}`;
}

/**
 * Whether a player with `utr` may join a band (decision 27: without a UTR, only unbanded
 * requests). The server checks again; this only greys out a button that would fail.
 */
export function fitsLevel(
  utr: number | null | undefined,
  min?: number | null,
  max?: number | null,
) {
  if (min == null && max == null) return true;
  if (utr == null) return false;
  return (min == null || utr >= min) && (max == null || utr <= max);
}

/** A band of `spread` either side of `utr`, inside the API's 1.00–16.50. */
export function bandAround(utr: number, spread = 1): { min: number; max: number } {
  const round = (value: number) => Math.round(value * 100) / 100;
  return { min: round(Math.max(1, utr - spread)), max: round(Math.min(16.5, utr + spread)) };
}

/** "1 spot left", "3 spots left". */
export function spotsLabel(open: number): string {
  return `${open} ${open === 1 ? "spot" : "spots"} left`;
}

/** "Sat 10 Oct, 09:00–12:00", or both ends in full when the window spans days. */
export function windowLabel(startIso: string, endIso: string, locale?: string): string {
  const start = new Date(startIso);
  const end = new Date(endIso);
  const day = new Intl.DateTimeFormat(locale, { weekday: "short", day: "numeric", month: "short" });
  const time = new Intl.DateTimeFormat(locale, { hour: "2-digit", minute: "2-digit" });
  if (day.format(start) === day.format(end)) {
    return `${day.format(start)}, ${time.format(start)}–${time.format(end)}`;
  }
  return `${day.format(start)} ${time.format(start)} – ${day.format(end)} ${time.format(end)}`;
}
