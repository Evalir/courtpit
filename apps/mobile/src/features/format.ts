import type { components } from "@courtpit/api-client";

type Discipline = components["schemas"]["Discipline"];
type PlayPref = components["schemas"]["PlayPref"];

export const disciplineLabel: Record<Discipline, string> = {
  singles: "Singles",
  doubles: "Doubles",
  mixed: "Mixed doubles",
};

export const playPrefLabel: Record<PlayPref, string> = {
  singles: "Singles",
  doubles: "Doubles",
  any: "Singles & doubles",
};

/** "Sat 11 Oct, 18:00" in the device's locale and time zone. */
export function formatDateTime(iso: string, locale?: string): string {
  return new Intl.DateTimeFormat(locale, {
    weekday: "short",
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(iso));
}

/** "11 Oct 2026". */
export function formatDate(iso: string, locale?: string): string {
  return new Intl.DateTimeFormat(locale, {
    day: "numeric",
    month: "short",
    year: "numeric",
  }).format(new Date(iso));
}

/** A UTR to two decimals ("6.25"), or "—" when unknown. */
export function formatUtr(utr: number | null | undefined): string {
  return utr === null || utr === undefined ? "—" : utr.toFixed(2);
}
