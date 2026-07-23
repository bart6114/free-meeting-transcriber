import { Trans } from "@lingui/react/macro";

import { ChangeLocationRow } from "./change-location";
import { ReExportAllFilesRow } from "./reexport-all";

export function StorageSettingsView() {
  return (
    <div>
      <h2 className="mb-4 font-sans text-lg font-semibold">
        <Trans>Storage</Trans>
      </h2>
      <p className="text-muted-foreground mb-3 text-xs">
        <Trans>
          Your data lives as files in this folder. The internal database is a
          cache and can be rebuilt at any time.
        </Trans>
      </p>
      <div className="flex flex-col gap-3">
        <ChangeLocationRow />
        <ReExportAllFilesRow />
      </div>
    </div>
  );
}
