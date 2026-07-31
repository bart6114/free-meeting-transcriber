import { Trans } from "@lingui/react/macro";

import { OnboardingButton } from "./shared";

import { ConfigureProviders } from "~/settings/ai/llm/configure";
import { LlmSettingsProvider } from "~/settings/ai/llm/context";
import { SelectProviderAndModel } from "~/settings/ai/llm/select";
import { useConfigValues } from "~/shared/config";

export function LlmProviderSection({
  onContinue,
}: {
  onContinue?: () => void;
}) {
  const { current_llm_provider, current_llm_model } = useConfigValues([
    "current_llm_provider",
    "current_llm_model",
  ] as const);
  const isConfigured = !!(current_llm_provider && current_llm_model);

  return (
    <LlmSettingsProvider>
      <div className="flex flex-col gap-6">
        <SelectProviderAndModel showAlerts={false} />
        <ConfigureProviders />
        <OnboardingButton
          disabled={!isConfigured}
          onClick={onContinue}
          className="disabled:cursor-default disabled:opacity-50"
        >
          <Trans>Continue</Trans>
        </OnboardingButton>
      </div>
    </LlmSettingsProvider>
  );
}
