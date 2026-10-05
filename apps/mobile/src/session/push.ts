import type { CourtpitClient } from "@courtpit/api-client";
import Constants from "expo-constants";
import * as Device from "expo-device";
import * as Notifications from "expo-notifications";
import { router, type Href } from "expo-router";
import { useEffect } from "react";
import { Platform } from "react-native";

import { notificationPath } from "./notificationPath";
import { returnTo } from "./returnTo";

/** Whether this device can show pushes, and whether the player let it. */
export type PushPermission = "granted" | "undetermined" | "denied" | "unavailable";

// Notifications that arrive while the app is open still show as a banner.
Notifications.setNotificationHandler({
  handleNotification: async () => ({
    shouldShowBanner: true,
    shouldShowList: true,
    shouldPlaySound: false,
    shouldSetBadge: false,
  }),
});

const projectId: string | undefined = Constants.expoConfig?.extra?.eas?.projectId;

/** The token this device registered for the signed-in player (forgotten on sign-out). */
let registered: string | null = null;

/** Push needs a physical device and an EAS project (simulators and Expo Go builds lack one). */
export async function pushPermission(): Promise<PushPermission> {
  if (!Device.isDevice || !projectId) return "unavailable";
  const settings = await Notifications.getPermissionsAsync();
  if (settings.granted) return "granted";
  return settings.canAskAgain ? "undetermined" : "denied";
}

/**
 * Registers this device for the signed-in player: asks first when `ask` (only from a tap on
 * "Turn on"), then sends the Expo push token to the server. Returns where permission stands.
 */
export async function registerForPush(
  fetch: CourtpitClient,
  ask: boolean,
): Promise<PushPermission> {
  let permission = await pushPermission();
  if (permission === "undetermined" && ask) {
    const answer = await Notifications.requestPermissionsAsync();
    permission = answer.granted ? "granted" : "denied";
  }
  if (permission !== "granted" || !projectId) return permission;
  if (Platform.OS === "android") {
    await Notifications.setNotificationChannelAsync("default", {
      name: "Matches and leagues",
      importance: Notifications.AndroidImportance.DEFAULT,
    });
  }
  const { data: token } = await Notifications.getExpoPushTokenAsync({ projectId });
  const { error } = await fetch.PUT("/api/v1/me/devices/{token}", {
    params: { path: { token } },
    body: { platform: Platform.OS === "android" ? "android" : "ios" },
  });
  if (error === undefined) registered = token;
  return permission;
}

/** Stops pushes to this device for the player signing out (best effort). */
export async function forgetPushDevice(fetch: CourtpitClient): Promise<void> {
  if (!registered) return;
  const token = registered;
  registered = null;
  await fetch.DELETE("/api/v1/me/devices/{token}", { params: { path: { token } } });
}

let openedLaunchNotification = false;

/**
 * Opens what a tapped notification is about: straight away when signed in, after sign-in
 * otherwise (through `returnTo`). Handles the notification that launched the app once.
 */
export function useNotificationTaps(signedIn: boolean) {
  useEffect(() => {
    const open = (data: unknown) => {
      const path = notificationPath(data);
      if (!path) return;
      if (signedIn) router.push(path as Href);
      else returnTo.remember(path);
    };
    if (!openedLaunchNotification) {
      openedLaunchNotification = true;
      const launch = Notifications.getLastNotificationResponse();
      if (launch) open(launch.notification.request.content.data);
    }
    const subscription = Notifications.addNotificationResponseReceivedListener((response) =>
      open(response.notification.request.content.data),
    );
    return () => subscription.remove();
  }, [signedIn]);
}
