import { useLingui } from "@lingui/react/macro";

import type { TodoProvider } from "./shared";

import {
  AccessPermissionRow,
  TroubleShootingLink,
} from "~/calendar/components/apple/permission";
import { usePermission } from "~/shared/hooks/usePermissions";

// GitHub/Linear OAuth todo providers were removed (Task 4 review fix) — see
// ./shared.tsx. Apple Reminders is the only remaining provider.
export function TodoProviderContent({ config: _config }: { config: TodoProvider }) {
  return <AppleRemindersProviderContent />;
}

function AppleRemindersProviderContent() {
  const { t } = useLingui();
  const reminders = usePermission("reminders");

  if (reminders.status !== "authorized") {
    return (
      <AccessPermissionRow
        title={t`Reminders`}
        status={reminders.status}
        isPending={reminders.isPending}
        onOpen={reminders.open}
        onRequest={reminders.request}
        onReset={reminders.reset}
      />
    );
  }

  return (
    <TroubleShootingLink
      onRequest={reminders.request}
      onReset={reminders.reset}
      onOpen={reminders.open}
      isPending={reminders.isPending}
    />
  );
}
