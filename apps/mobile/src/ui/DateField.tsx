import { useState } from "react";
import { Pressable, View } from "react-native";

import { dayOf, formatDay, monthGrid, startOf, type Day } from "./calendar";
import { createStyles } from "@/theme/ThemeProvider";
import { radius, space } from "@/theme/tokens";

import { Button } from "./Button";
import { Text } from "./Text";

const WEEKDAYS = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

/** A labelled calendar day: the chosen day, and a month to pick from when opened. */
export function DateField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: Day;
  onChange: (day: Day) => void;
}) {
  const styles = useStyles();
  const [open, setOpen] = useState(false);
  const [month, setMonth] = useState(() => {
    const date = startOf(value);
    return { year: date.getFullYear(), month: date.getMonth() };
  });
  const shift = (by: number) => {
    const date = new Date(month.year, month.month + by, 1);
    setMonth({ year: date.getFullYear(), month: date.getMonth() });
  };
  const title = new Intl.DateTimeFormat(undefined, { month: "long", year: "numeric" }).format(
    new Date(month.year, month.month, 1),
  );

  return (
    <View style={{ gap: space.xs }}>
      <Text variant="label" tone="textMuted">
        {label}
      </Text>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={`${label}: ${formatDay(value)}`}
        onPress={() => setOpen(!open)}
        style={styles.value}
      >
        <Text>{formatDay(value)}</Text>
      </Pressable>
      {open ? (
        <View style={styles.calendar}>
          <View style={styles.header}>
            <Button label="‹" variant="ghost" size="sm" onPress={() => shift(-1)} />
            <Text variant="label" weight="semibold">
              {title}
            </Text>
            <Button label="›" variant="ghost" size="sm" onPress={() => shift(1)} />
          </View>
          <View style={styles.week}>
            {WEEKDAYS.map((weekday) => (
              <Text key={weekday} variant="caption" tone="textMuted" style={styles.cell}>
                {weekday}
              </Text>
            ))}
          </View>
          {monthGrid(month.year, month.month).map((week, row) => (
            <View key={row} style={styles.week}>
              {week.map((day, column) =>
                day ? (
                  <Pressable
                    key={day}
                    accessibilityRole="button"
                    accessibilityLabel={formatDay(day)}
                    accessibilityState={{ selected: day === value }}
                    onPress={() => {
                      onChange(day);
                      setOpen(false);
                    }}
                    style={[styles.cell, styles.day, day === value && styles.selected]}
                  >
                    <Text
                      variant="label"
                      tone={day === value ? "onPrimary" : "text"}
                      weight={day === dayOf(new Date()) ? "bold" : "regular"}
                    >
                      {Number(day.slice(8))}
                    </Text>
                  </Pressable>
                ) : (
                  <View key={`blank-${column}`} style={styles.cell} />
                ),
              )}
            </View>
          ))}
        </View>
      ) : null}
    </View>
  );
}

const useStyles = createStyles(({ colors }) => ({
  value: {
    minHeight: 48,
    justifyContent: "center",
    paddingHorizontal: space.md,
    borderRadius: radius.md,
    borderWidth: 1,
    borderColor: colors.border,
    backgroundColor: colors.surface,
  },
  calendar: {
    padding: space.sm,
    gap: space.xxs,
    borderRadius: radius.md,
    borderWidth: 1,
    borderColor: colors.border,
    backgroundColor: colors.surface,
  },
  header: { flexDirection: "row", alignItems: "center", justifyContent: "space-between" },
  week: { flexDirection: "row" },
  cell: { flex: 1, textAlign: "center", alignItems: "center", justifyContent: "center" },
  day: { height: 44, borderRadius: radius.pill },
  selected: { backgroundColor: colors.primary },
}));
