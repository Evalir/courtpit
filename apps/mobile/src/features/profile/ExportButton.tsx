import { useMutation } from "@tanstack/react-query";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { Button } from "@/ui/Button";
import { Text } from "@/ui/Text";

import { exportFileName } from "./account";
import { saveExport } from "./saveExport";

/** Downloads (web) or shares (native) everything the account holds, as JSON. */
export function ExportButton() {
  const { fetch } = useApi();
  const download = useMutation({
    mutationFn: async () => {
      const { data, error } = await fetch.GET("/api/v1/me/export");
      if (error !== undefined) throw error;
      await saveExport(exportFileName(new Date()), JSON.stringify(data, null, 2));
    },
  });
  return (
    <>
      <Button
        label="Download my data"
        variant="secondary"
        icon="download-outline"
        block
        loading={download.isPending}
        onPress={() => download.mutate()}
      />
      {download.error ? <Text tone="danger">{describeError(download.error)}</Text> : null}
    </>
  );
}
