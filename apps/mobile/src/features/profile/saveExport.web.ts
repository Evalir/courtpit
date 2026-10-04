/** Downloads the export as a JSON file. */
export async function saveExport(fileName: string, json: string): Promise<void> {
  const url = URL.createObjectURL(new Blob([json], { type: "application/json" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  link.click();
  URL.revokeObjectURL(url);
}
