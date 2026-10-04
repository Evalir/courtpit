import { ScrollView, View } from "react-native";

import { space } from "@/theme/tokens";
import { Chip } from "@/ui/Chip";
import { Text } from "@/ui/Text";

import { dayLabel, daySlots, sameDay, timeLabel, upcomingDays } from "./when";

/** Pick a day (next four weeks), then a half-hour start time on it. */
export function WhenPicker({
  day,
  time,
  now,
  onDay,
  onTime,
}: {
  day: Date;
  time: Date | null;
  now: Date;
  onDay: (day: Date) => void;
  onTime: (time: Date) => void;
}) {
  return (
    <View style={{ gap: space.md }}>
      <Text variant="label" tone="textMuted">
        Day
      </Text>
      <ScrollView
        horizontal
        showsHorizontalScrollIndicator={false}
        contentContainerStyle={{ gap: space.sm }}
      >
        {upcomingDays(now).map((candidate) => (
          <Chip
            key={candidate.toISOString()}
            label={dayLabel(candidate, now)}
            selected={sameDay(candidate, day)}
            onPress={() => onDay(candidate)}
          />
        ))}
      </ScrollView>
      <Text variant="label" tone="textMuted">
        Start time
      </Text>
      <View style={{ flexDirection: "row", flexWrap: "wrap", gap: space.sm }}>
        {daySlots(day, now).map((slot) => (
          <Chip
            key={slot.time.toISOString()}
            label={timeLabel(slot.time)}
            disabled={slot.past}
            selected={time !== null && slot.time.getTime() === time.getTime()}
            onPress={() => onTime(slot.time)}
          />
        ))}
      </View>
    </View>
  );
}
