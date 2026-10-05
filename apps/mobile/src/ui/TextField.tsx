import { useState } from "react";
import { TextInput, View, type TextInputProps } from "react-native";

import { fontFamilies } from "@/theme/fonts";
import { createStyles, useTheme } from "@/theme/ThemeProvider";
import { radius, space, typeScale } from "@/theme/tokens";

import { Text } from "./Text";

export interface TextFieldProps extends TextInputProps {
  label: string;
  /** Help under the field; replaced by `error` when there is one. */
  hint?: string;
  error?: string | null;
}

/** A labelled text input with hint and error text. */
export function TextField({
  label,
  hint,
  error,
  style,
  onFocus,
  onBlur,
  ...props
}: TextFieldProps) {
  const styles = useStyles();
  const { colors, typography } = useTheme();
  const [focused, setFocused] = useState(false);
  return (
    <View style={styles.field}>
      <Text variant="label" tone="textMuted">
        {label}
      </Text>
      <TextInput
        accessibilityLabel={label}
        placeholderTextColor={colors.textMuted}
        {...props}
        onFocus={(event) => {
          setFocused(true);
          onFocus?.(event);
        }}
        onBlur={(event) => {
          setFocused(false);
          onBlur?.(event);
        }}
        style={[
          styles.input,
          { fontFamily: fontFamilies[typography].regular },
          focused && styles.focused,
          error ? styles.invalid : null,
          style,
        ]}
      />
      {error || hint ? (
        <Text variant="caption" tone={error ? "danger" : "textMuted"}>
          {error || hint}
        </Text>
      ) : null}
    </View>
  );
}

const useStyles = createStyles(({ colors }) => ({
  field: { gap: space.xs, alignSelf: "stretch" },
  input: {
    minHeight: 48,
    paddingHorizontal: space.md,
    borderRadius: radius.md,
    borderWidth: 1,
    borderColor: colors.border,
    backgroundColor: colors.surface,
    color: colors.text,
    fontSize: typeScale.body.fontSize,
  },
  focused: { borderColor: colors.primary, borderWidth: 2, paddingHorizontal: space.md - 1 },
  invalid: { borderColor: colors.danger },
}));
