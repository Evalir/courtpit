/** Home's one-line summary: "1 match and 2 league entries need you." */
export function needsYouLine(matches: number, entries: number): string {
  if (matches + entries === 0) return "You’re all caught up.";
  const parts: string[] = [];
  if (matches > 0) parts.push(`${matches} ${matches === 1 ? "match" : "matches"}`);
  if (entries > 0) parts.push(`${entries} league ${entries === 1 ? "entry" : "entries"}`);
  return `${parts.join(" and ")} ${matches + entries === 1 ? "needs" : "need"} you.`;
}
