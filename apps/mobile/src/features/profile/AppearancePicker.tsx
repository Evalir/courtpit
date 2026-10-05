import { appearanceOptions } from "@/theme/appearance";
import { useAppearance } from "@/theme/AppearanceProvider";
import { Segmented } from "@/ui/Segmented";
import { Text } from "@/ui/Text";

/** Light, dark or the system's: a per-device choice, so it sits outside the account. */
export function AppearancePicker() {
  const { preference, setPreference } = useAppearance();
  return (
    <>
      <Segmented options={appearanceOptions} value={preference} onChange={setPreference} />
      <Text variant="caption" tone="textMuted">
        {preference === "system"
          ? "Follows this device's light or dark setting."
          : "On this device only."}
      </Text>
    </>
  );
}
