import { createFileRoute } from "@tanstack/react-router";
import { useCallback } from "react";

import { dismissInstruction } from "@hypr/plugin-windows";

import { InstructionScreen } from "~/instruction";

// The only surviving flow that opens this window is third-party OAuth
// (calendar/todo) integration — "sign-in" and "billing" instruction types
// were removed with accounts/billing (Task 4).
export const Route = createFileRoute("/app/instruction")({
  validateSearch: (
    search,
  ): { url?: string; integrationId?: string } => ({
    url: (search as { url?: string }).url,
    integrationId: (search as { integrationId?: string }).integrationId,
  }),
  component: InstructionRoute,
});

function useHandleBack() {
  return useCallback(() => dismissInstruction(), []);
}

function InstructionRoute() {
  const { url, integrationId } = Route.useSearch();
  const handleBack = useHandleBack();
  const onBack = useCallback(() => void handleBack(), [handleBack]);

  return (
    <InstructionScreen url={url} integrationId={integrationId} onBack={onBack} />
  );
}
