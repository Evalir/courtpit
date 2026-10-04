import { Share } from "react-native";

/** Hands the export to the system share sheet (save to Files, mail it, …). */
export async function saveExport(fileName: string, json: string): Promise<void> {
  await Share.share({ title: fileName, message: json });
}
