import { View } from "react-native";

/** A tennis ball drawn with views: a disc and two seam arcs (no image assets needed). */
export function TennisBall({ size, color }: { size: number; color: string }) {
  const seam = {
    position: "absolute" as const,
    width: size * 1.3,
    height: size * 1.3,
    top: -size * 0.15,
    borderRadius: size * 0.65,
    borderWidth: Math.max(2, size * 0.035),
    borderColor: "rgba(255, 255, 255, 0.85)",
  };
  return (
    <View
      accessibilityElementsHidden
      importantForAccessibility="no-hide-descendants"
      style={{
        width: size,
        height: size,
        borderRadius: size / 2,
        backgroundColor: color,
        overflow: "hidden",
      }}
    >
      <View style={[seam, { left: -size * 0.98 }]} />
      <View style={[seam, { left: size * 0.68 }]} />
    </View>
  );
}
