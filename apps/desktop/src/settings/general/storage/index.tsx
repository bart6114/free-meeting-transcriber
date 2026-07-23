import { Trans } from "@lingui/react/macro";

import { ChangeLocationRow } from "./change-location";

export function StorageSettingsView() {
  return (
    <div>
      <h2 className="mb-4 font-sans text-lg font-semibold">
        <Trans>Storage</Trans>
      </h2>
      <div className="flex flex-col gap-3">
        <ChangeLocationRow />
      </div>
    </div>
  );
}
