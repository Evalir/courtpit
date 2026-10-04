import { createContext, use, type ReactNode } from "react";
import { StyleSheet } from "react-native";

import { themeFromBranding, type Theme } from "./theme";

const ThemeContext = createContext<Theme>(themeFromBranding(undefined));

/** Provides the community's theme to everything below it. */
export function ThemeProvider({ theme, children }: { theme: Theme; children: ReactNode }) {
  return <ThemeContext value={theme}>{children}</ThemeContext>;
}

/** The current community's theme. */
export function useTheme(): Theme {
  return use(ThemeContext);
}

/**
 * Declares theme-dependent styles once, outside the component:
 *
 * ```ts
 * const useStyles = createStyles(({ colors }) => ({ card: { backgroundColor: colors.surface } }));
 * ```
 *
 * The hook builds the `StyleSheet` once per theme object and reuses it on every render.
 */
export function createStyles<T extends StyleSheet.NamedStyles<T>>(factory: (theme: Theme) => T) {
  const cache = new WeakMap<Theme, T>();
  return function useStyles(): T {
    const theme = useTheme();
    let styles = cache.get(theme);
    if (!styles) {
      styles = StyleSheet.create(factory(theme));
      cache.set(theme, styles);
    }
    return styles;
  };
}
