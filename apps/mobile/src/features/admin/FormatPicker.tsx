import type { components } from "@courtpit/api-client";
import { View } from "react-native";

import { describeFormat } from "@/features/leagues/league";
import { space } from "@/theme/tokens";
import { Segmented } from "@/ui/Segmented";
import { Text } from "@/ui/Text";
import { Toggle } from "@/ui/Toggle";

import { STARTER_FORMAT, type FormatChoice } from "./leagueForm";

type MatchFormat = components["schemas"]["MatchFormat"];

/** The league's match format: as it is, the club's default, or its own. */
export function FormatPicker({
  value,
  current,
  onChange,
}: {
  value: FormatChoice;
  /** The league's format today, when editing (offers "keep"). */
  current?: MatchFormat;
  onChange: (choice: FormatChoice) => void;
}) {
  const kinds = [
    ...(current ? [{ value: "keep" as const, label: "As it is" }] : []),
    { value: "default" as const, label: "Club default" },
    { value: "custom" as const, label: "Custom" },
  ];
  const choose = (kind: FormatChoice["kind"]) =>
    onChange(kind === "custom" ? { kind, format: current ?? STARTER_FORMAT } : { kind });
  const format = value.kind === "custom" ? value.format : null;
  const set = (patch: Partial<MatchFormat>) =>
    format && onChange({ kind: "custom", format: { ...format, ...patch } });

  return (
    <View style={{ gap: space.md }}>
      <Segmented options={kinds} value={value.kind} onChange={choose} />
      {value.kind === "keep" && current ? (
        <Text variant="caption" tone="textMuted">
          {describeFormat(current)}
        </Text>
      ) : null}
      {format ? (
        <>
          <Segmented
            options={[
              { value: "1", label: "One set" },
              { value: "2", label: "Best of 3" },
              { value: "3", label: "Best of 5" },
            ]}
            value={String(format.sets_to_win)}
            onChange={(sets) => set({ sets_to_win: Number(sets) })}
          />
          {format.sets_to_win > 1 ? (
            <View style={{ gap: space.xs }}>
              <Text variant="label" tone="textMuted">
                Deciding set
              </Text>
              <Segmented
                options={[
                  { value: "full_set" as const, label: "Full set" },
                  { value: "match_tiebreak_10" as const, label: "Tiebreak to 10" },
                  { value: "pro_set_8" as const, label: "Pro set" },
                ]}
                value={format.final_set}
                onChange={(final_set) => set({ final_set })}
              />
            </View>
          ) : null}
          <Toggle
            label="Tiebreak at 6–6"
            hint="Off means advantage sets."
            value={format.tiebreak_at != null}
            onChange={(on) => set({ tiebreak_at: on ? format.games_per_set : null })}
          />
          <Toggle
            label="Golden point at deuce"
            value={format.deuce === "golden_point"}
            onChange={(on) => set({ deuce: on ? "golden_point" : "advantage" })}
          />
          <Text variant="caption" tone="textMuted">
            {describeFormat(format)}
          </Text>
        </>
      ) : null}
    </View>
  );
}
