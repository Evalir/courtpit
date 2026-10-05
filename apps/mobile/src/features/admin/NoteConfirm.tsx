import { useState } from "react";
import { View } from "react-native";

import { space } from "@/theme/tokens";
import { Button } from "@/ui/Button";
import { Text } from "@/ui/Text";
import { TextField } from "@/ui/TextField";

/** An admin decision's second step: what it does, an optional note for the players, go or back. */
export function NoteConfirm({
  prompt,
  confirmLabel,
  danger = false,
  loading,
  onConfirm,
  onBack,
  children,
}: {
  prompt: string;
  confirmLabel: string;
  danger?: boolean;
  loading: boolean;
  onConfirm: (note: string | null) => void;
  onBack: () => void;
  /** Extra choices above the note (e.g. which side wins). */
  children?: React.ReactNode;
}) {
  const [note, setNote] = useState("");
  return (
    <View style={{ gap: space.sm }}>
      <Text variant="label">{prompt}</Text>
      {children}
      <TextField
        label="Note for the players (optional)"
        value={note}
        onChangeText={setNote}
        multiline
      />
      <View style={{ flexDirection: "row", gap: space.sm }}>
        <Button
          label={confirmLabel}
          variant={danger ? "danger" : "primary"}
          size="sm"
          loading={loading}
          onPress={() => onConfirm(note.trim() || null)}
        />
        <Button label="Back" variant="ghost" size="sm" onPress={onBack} />
      </View>
    </View>
  );
}
