/**
 * The day-and-slot picker behind "Propose a time": the same on iOS, Android and the web
 * (React Native ships no cross-platform date picker). Times are the device's local time.
 */

/** How far ahead a proposal can be picked, and the playing day's slots. */
export const pickerWindow = { days: 28, firstHour: 7, lastHour: 22, stepMinutes: 30 } as const;

/** Local midnight of `date`. */
export function startOfDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

/** `count` days from today, as local midnights. */
export function upcomingDays(now: Date, count: number = pickerWindow.days): Date[] {
  const today = startOfDay(now);
  return Array.from(
    { length: count },
    (_unused, offset) => new Date(today.getFullYear(), today.getMonth(), today.getDate() + offset),
  );
}

/** A day's start times from `firstHour` to the last slot before `lastHour`, past ones marked. */
export function daySlots(day: Date, now: Date): { time: Date; past: boolean }[] {
  const slots: { time: Date; past: boolean }[] = [];
  const { firstHour, lastHour, stepMinutes } = pickerWindow;
  for (let minutes = firstHour * 60; minutes < lastHour * 60; minutes += stepMinutes) {
    const time = new Date(
      day.getFullYear(),
      day.getMonth(),
      day.getDate(),
      Math.floor(minutes / 60),
      minutes % 60,
    );
    slots.push({ time, past: time <= now });
  }
  return slots;
}

/** "Today", "Tomorrow", else "Sat 11". */
export function dayLabel(day: Date, now: Date, locale?: string): string {
  const days = Math.round((startOfDay(day).getTime() - startOfDay(now).getTime()) / 86_400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Tomorrow";
  return new Intl.DateTimeFormat(locale, { weekday: "short", day: "numeric" }).format(day);
}

/** "18:30" (or "6:30 PM", by locale). */
export function timeLabel(time: Date, locale?: string): string {
  return new Intl.DateTimeFormat(locale, { hour: "2-digit", minute: "2-digit" }).format(time);
}

/** Whether two instants fall on the same local day. */
export function sameDay(one: Date, other: Date): boolean {
  return startOfDay(one).getTime() === startOfDay(other).getTime();
}
