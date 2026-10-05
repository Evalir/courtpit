/** Calendar days as `YYYY-MM-DD` in the device's time zone. */
export type Day = string;

const pad = (value: number) => String(value).padStart(2, "0");

/** The local calendar day of an instant. */
export function dayOf(instant: Date): Day {
  return `${instant.getFullYear()}-${pad(instant.getMonth() + 1)}-${pad(instant.getDate())}`;
}

function parts(day: Day): [number, number, number] {
  const [year = 0, month = 1, date = 1] = day.split("-").map(Number);
  return [year, month - 1, date];
}

/** Local midnight at the start of `day`. */
export function startOf(day: Day): Date {
  const [year, month, date] = parts(day);
  return new Date(year, month, date);
}

/** Local midnight at the end of `day` (the start of the next). */
export function endOf(day: Day): Date {
  const [year, month, date] = parts(day);
  return new Date(year, month, date + 1);
}

/** `day` moved by `days` (negative goes back). */
export function addDays(day: Day, days: number): Day {
  const [year, month, date] = parts(day);
  return dayOf(new Date(year, month, date + days));
}

/** The last day an end-of-day instant belongs to (`endOf` reversed). */
export function lastDayBefore(instant: Date): Day {
  return dayOf(new Date(instant.getTime() - 1));
}

/** A month's days as weeks starting on Monday; blanks before the 1st and after the last. */
export function monthGrid(year: number, month: number): (Day | null)[][] {
  const first = new Date(year, month, 1);
  const days = new Date(year, month + 1, 0).getDate();
  const lead = (first.getDay() + 6) % 7;
  const cells: (Day | null)[] = Array.from({ length: lead }, () => null);
  for (let date = 1; date <= days; date += 1) cells.push(dayOf(new Date(year, month, date)));
  while (cells.length % 7 !== 0) cells.push(null);
  const weeks: (Day | null)[][] = [];
  for (let index = 0; index < cells.length; index += 7) weeks.push(cells.slice(index, index + 7));
  return weeks;
}

/** "Sat, 10 Oct 2026" (en-GB). */
export function formatDay(day: Day, locale?: string): string {
  return new Intl.DateTimeFormat(locale, {
    weekday: "short",
    day: "numeric",
    month: "short",
    year: "numeric",
  }).format(startOf(day));
}
