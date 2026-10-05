import Constants from "expo-constants";
import { Platform } from "react-native";

/** How a new session's device is listed (`device_label`): the phone's name, or the platform. */
export const deviceLabel =
  Constants.deviceName ?? (Platform.OS === "web" ? "Web browser" : `${Platform.OS} app`);
