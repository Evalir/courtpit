import { router, type Href } from "expo-router";

/**
 * Closes a modal screen after its write. A modal opened straight from a link (web) has nothing
 * behind it to go back to, so it gives way to `fallback` instead.
 */
export function closeModal(fallback: Href): void {
  if (router.canGoBack()) router.back();
  else router.replace(fallback);
}
