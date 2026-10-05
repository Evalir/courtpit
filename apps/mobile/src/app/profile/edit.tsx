import type { components } from "@racquetcollective/api-client";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { refreshAfterWrite } from "@/api/queryClient";
import { playPrefLabel } from "@/features/format";
import { closeModal } from "@/features/navigation";
import {
  NETWORKS,
  checkProfile,
  formFromProfile,
  profilePatch,
  type ProfileForm,
} from "@/features/profile/profileForm";
import { space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
import { Chip } from "@/ui/Chip";
import { Screen, Section } from "@/ui/Screen";
import { Segmented } from "@/ui/Segmented";
import { ErrorState, LoadingState } from "@/ui/States";
import { Text } from "@/ui/Text";
import { TextField } from "@/ui/TextField";
import { Toggle } from "@/ui/Toggle";

type PlayerProfile = components["schemas"]["PlayerProfile"];
type Gender = components["schemas"]["Gender"];

const GENDERS: { value: Gender; label: string }[] = [
  { value: "female", label: "Female" },
  { value: "male", label: "Male" },
  { value: "other", label: "Other" },
  { value: "undisclosed", label: "Not saying" },
];

/** Edit profile: name, level, play preferences, places, gear and contact details. */
export default function EditProfile() {
  const { $api } = useApi();
  const me = $api.useQuery("get", "/api/v1/me");
  if (me.isPending) return <LoadingState />;
  if (me.error) return <ErrorState error={me.error} onRetry={() => void me.refetch()} />;
  return <ProfileEditor profile={me.data.player} />;
}

function ProfileEditor({ profile }: { profile: PlayerProfile }) {
  const { $api } = useApi();
  const queryClient = useQueryClient();
  const [form, setForm] = useState(() => formFromProfile(profile));
  const [place, setPlace] = useState("");
  const [tried, setTried] = useState(false);
  const save = $api.useMutation("patch", "/api/v1/me", {
    onSuccess: async () => {
      await refreshAfterWrite(queryClient);
      closeModal("/profile");
    },
  });
  const set = <K extends keyof ProfileForm>(key: K, value: ProfileForm[K]) =>
    setForm((current) => ({ ...current, [key]: value }));
  const problems = checkProfile(form);
  const shown = tried ? problems : {};
  const patch = profilePatch(profile, form);
  const changed = Object.keys(patch).length > 0;
  const addPlace = () => {
    const name = place.trim();
    if (name !== "" && !form.preferred_locations.includes(name)) {
      set("preferred_locations", [...form.preferred_locations, name]);
    }
    setPlace("");
  };

  return (
    <Screen>
      <Section title="About you">
        <TextField
          label="Name"
          value={form.display_name}
          onChangeText={(value) => set("display_name", value)}
          error={shown.display_name}
          autoComplete="name"
        />
        <TextField
          label="UTR"
          value={form.utr}
          onChangeText={(value) => set("utr", value)}
          error={shown.utr}
          hint="Your Universal Tennis Rating. It places you in league boxes and match requests."
          keyboardType="decimal-pad"
          placeholder="e.g. 5.80"
        />
        <View style={{ gap: space.sm }}>
          <Text variant="label" tone="textMuted">
            Gender
          </Text>
          <Segmented
            options={GENDERS}
            value={form.gender}
            onChange={(value) => set("gender", value)}
          />
          <Text variant="caption" tone="textMuted">
            Only used to pair mixed doubles; never shown to other members.
          </Text>
        </View>
        <View style={{ gap: space.sm }}>
          <Text variant="label" tone="textMuted">
            Plays
          </Text>
          <Segmented
            options={(["singles", "doubles", "any"] as const).map((value) => ({
              value,
              label: playPrefLabel[value],
            }))}
            value={form.play_pref}
            onChange={(value) => set("play_pref", value)}
          />
        </View>
      </Section>
      <Section title="Where you like to play">
        {form.preferred_locations.length > 0 ? (
          <View style={{ flexDirection: "row", flexWrap: "wrap", gap: space.sm }}>
            {form.preferred_locations.map((name) => (
              <Chip
                key={name}
                label={`${name} ✕`}
                accessibilityLabel={`Remove ${name}`}
                onPress={() =>
                  set(
                    "preferred_locations",
                    form.preferred_locations.filter((other) => other !== name),
                  )
                }
              />
            ))}
          </View>
        ) : null}
        <TextField
          label="Add a place"
          value={place}
          onChangeText={setPlace}
          onSubmitEditing={addPlace}
          error={shown.preferred_locations}
          hint="Suggested when you propose a time or post a match request."
          returnKeyType="done"
        />
        {place.trim() !== "" ? (
          <Button label={`Add “${place.trim()}”`} variant="secondary" onPress={addPlace} />
        ) : null}
      </Section>
      <Section title="Gear">
        <TextField
          label="Racket"
          value={form.racket}
          onChangeText={(value) => set("racket", value)}
          error={shown.racket}
        />
        <TextField
          label="Strings"
          value={form.strings}
          onChangeText={(value) => set("strings", value)}
          error={shown.strings}
        />
        <TextField
          label="Tension (kg)"
          value={form.tension_kg}
          onChangeText={(value) => set("tension_kg", value)}
          error={shown.tension_kg}
          keyboardType="decimal-pad"
        />
      </Section>
      <ContactFields form={form} set={set} problems={shown} />
      {save.error ? <Text tone="danger">{describeError(save.error)}</Text> : null}
      {tried && Object.keys(problems).length > 0 ? (
        <Text tone="danger">Check the highlighted fields.</Text>
      ) : null}
      <Button
        label="Save"
        block
        disabled={!changed}
        loading={save.isPending}
        onPress={() => {
          setTried(true);
          if (Object.keys(problems).length === 0) save.mutate({ body: patch });
        }}
      />
    </Screen>
  );
}

function ContactFields({
  form,
  set,
  problems,
}: {
  form: ProfileForm;
  set: <K extends keyof ProfileForm>(key: K, value: ProfileForm[K]) => void;
  problems: Partial<Record<keyof ProfileForm, string>>;
}) {
  return (
    <Section title="Contact">
      <TextField
        label="Phone"
        value={form.phone}
        onChangeText={(value) => set("phone", value)}
        error={problems.phone}
        keyboardType="phone-pad"
        autoComplete="tel"
      />
      <Toggle
        label="Show my phone to members"
        hint="Verified members of the club can see it on your profile."
        value={form.phone_visible}
        onChange={(value) => set("phone_visible", value)}
      />
      {NETWORKS.map(({ key, label }) => (
        <TextField
          key={key}
          label={label}
          value={form.socials[key]}
          onChangeText={(value) => set("socials", { ...form.socials, [key]: value })}
          autoCapitalize="none"
          autoCorrect={false}
        />
      ))}
      {problems.socials ? <Text tone="danger">{problems.socials}</Text> : null}
      <Toggle
        label="Show my socials to members"
        value={form.socials_visible}
        onChange={(value) => set("socials_visible", value)}
      />
    </Section>
  );
}
