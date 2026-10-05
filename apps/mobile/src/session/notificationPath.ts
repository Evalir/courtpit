import { returnPathOf } from "./returnTo";

/** The app path a notification opens (`data.url` from the server), if it names one. */
export function notificationPath(data: unknown): string | null {
  if (data == null || typeof data !== "object" || !("url" in data)) return null;
  const { url } = data;
  return typeof url === "string" ? returnPathOf(url) : null;
}
