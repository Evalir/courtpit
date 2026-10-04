import { Tabs } from "expo-router";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { fontFamilies } from "@/theme/fonts";
import { useTheme } from "@/theme/ThemeProvider";
import { space } from "@/theme/tokens";
import { Icon, type IconName } from "@/ui/Icon";

/** Ionicons pair per tab: outline when idle, filled when selected. */
function tabIcon(idle: IconName, selected: IconName) {
  return function TabIcon({ focused }: { focused: boolean }) {
    return (
      <Icon
        name={focused ? selected : idle}
        size={24}
        tone={focused ? "primaryText" : "textMuted"}
      />
    );
  };
}

/** The five destinations of the app (docs/frontend.md, Information architecture). */
export default function TabsLayout() {
  const { colors, typography } = useTheme();
  const insets = useSafeAreaInsets();
  return (
    <Tabs
      screenOptions={{
        // Each tab draws its own large title (`Screen`'s `title`), so no navigation header.
        headerShown: false,
        tabBarActiveTintColor: colors.primaryText,
        tabBarInactiveTintColor: colors.textMuted,
        // UIKit's 49 pt bar is too short for icon + label in some typefaces (Inter clips).
        tabBarStyle: {
          backgroundColor: colors.surface,
          borderTopColor: colors.border,
          height: 58 + insets.bottom,
          paddingTop: space.xs,
        },
        // An explicit line height keeps descenders ("Play", "Rankings") from being clipped.
        tabBarLabelStyle: {
          fontFamily: fontFamilies[typography].medium,
          fontWeight: "500",
          fontSize: 11,
          lineHeight: 16,
        },
        sceneStyle: { backgroundColor: colors.background },
      }}
    >
      <Tabs.Screen
        name="index"
        options={{ title: "Home", tabBarIcon: tabIcon("home-outline", "home") }}
      />
      <Tabs.Screen
        name="play"
        options={{ title: "Play", tabBarIcon: tabIcon("tennisball-outline", "tennisball") }}
      />
      <Tabs.Screen
        name="leagues"
        options={{ title: "Leagues", tabBarIcon: tabIcon("trophy-outline", "trophy") }}
      />
      <Tabs.Screen
        name="rankings"
        options={{ title: "Rankings", tabBarIcon: tabIcon("podium-outline", "podium") }}
      />
      <Tabs.Screen
        name="profile"
        options={{
          title: "Profile",
          tabBarIcon: tabIcon("person-circle-outline", "person-circle"),
        }}
      />
    </Tabs>
  );
}
