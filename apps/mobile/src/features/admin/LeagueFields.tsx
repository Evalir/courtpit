import type { components } from "@racquetcollective/api-client";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { useCommunity } from "@/tenant/TenantProvider";
import { space } from "@/theme/tokens";
import { Chip } from "@/ui/Chip";
import { DateField } from "@/ui/DateField";
import { Section } from "@/ui/Screen";
import { Segmented } from "@/ui/Segmented";
import { Stepper } from "@/ui/Stepper";
import { Text } from "@/ui/Text";
import { TextField } from "@/ui/TextField";

import { FormatPicker } from "./FormatPicker";
import type { LeagueForm, LeagueProblems } from "./leagueForm";

type MatchFormat = components["schemas"]["MatchFormat"];

/** The fields of the create and edit forms. */
export function LeagueFields({
  form,
  set,
  problems,
  creating,
  currentFormat,
}: {
  form: LeagueForm;
  set: <K extends keyof LeagueForm>(key: K, value: LeagueForm[K]) => void;
  problems: LeagueProblems;
  creating: boolean;
  currentFormat?: MatchFormat;
}) {
  const { features } = useCommunity();
  const disciplines = [
    { value: "singles" as const, label: "Singles" },
    ...(features.doubles ? [{ value: "doubles" as const, label: "Doubles" }] : []),
    ...(features.mixed ? [{ value: "mixed" as const, label: "Mixed" }] : []),
  ];
  return (
    <>
      <Section title="League">
        <TextField
          label="Name"
          value={form.name}
          onChangeText={(name) => set("name", name)}
          error={problems.name}
          placeholder="e.g. Winter Singles 2027"
        />
        {creating && disciplines.length > 1 ? (
          <Segmented
            options={disciplines}
            value={form.discipline}
            onChange={(discipline) => {
              set("discipline", discipline);
              set("previousLeagueId", null);
            }}
          />
        ) : null}
        <PreviousSeason form={form} set={set} />
      </Section>
      <Section title="Dates">
        <DateField
          label="Registration opens"
          value={form.opens}
          onChange={(day) => set("opens", day)}
        />
        <DateField
          label="Last day to register"
          value={form.closes}
          onChange={(day) => set("closes", day)}
        />
        <DateField
          label="Season starts"
          value={form.starts}
          onChange={(day) => set("starts", day)}
        />
        <DateField
          label="Last day of the season"
          value={form.ends}
          onChange={(day) => set("ends", day)}
        />
        {problems.dates ? <Text tone="danger">{problems.dates}</Text> : null}
        <Text variant="caption" tone="textMuted">
          Boxes are drawn when the season starts; unplayed matches are cancelled when it ends.
        </Text>
      </Section>
      <Section title="Boxes">
        <Stepper
          label="Smallest box"
          value={form.boxMin}
          min={2}
          max={16}
          onChange={(value) => set("boxMin", value)}
        />
        <Stepper
          label="Largest box"
          value={form.boxMax}
          min={2}
          max={16}
          onChange={(value) => set("boxMax", value)}
        />
        {problems.boxes ? <Text tone="danger">{problems.boxes}</Text> : null}
      </Section>
      <Section title="Match format">
        <FormatPicker
          value={form.format}
          current={currentFormat}
          onChange={(format) => set("format", format)}
        />
      </Section>
    </>
  );
}

/** "Continues from": a finished season of the same discipline seeds the boxes (promotion). */
function PreviousSeason({
  form,
  set,
}: {
  form: LeagueForm;
  set: <K extends keyof LeagueForm>(key: K, value: LeagueForm[K]) => void;
}) {
  const { $api } = useApi();
  const finished = $api.useQuery("get", "/api/v1/leagues", {
    params: { query: { status: "finished", discipline: form.discipline, limit: 20 } },
  });
  const options = finished.data?.items ?? [];
  if (options.length === 0) return null;
  return (
    <View style={{ gap: space.xs }}>
      <Text variant="label" tone="textMuted">
        Continues from (promotion and relegation place the boxes)
      </Text>
      <View style={{ flexDirection: "row", flexWrap: "wrap", gap: space.sm }}>
        <Chip
          label="A new league"
          selected={form.previousLeagueId === null}
          onPress={() => set("previousLeagueId", null)}
        />
        {options.map((league) => (
          <Chip
            key={league.id}
            label={league.name}
            selected={form.previousLeagueId === league.id}
            onPress={() => set("previousLeagueId", league.id)}
          />
        ))}
      </View>
    </View>
  );
}
