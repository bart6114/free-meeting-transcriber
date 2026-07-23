import { PROVIDERS } from "~/settings/ai/llm/shared";
import { getProviderSelectionBlockers } from "~/settings/ai/shared/eligibility";
import { getStoredAiProvider } from "~/settings/providers";
import { getStoredSettingValues, setSettingValues } from "~/settings/queries";
import type { SettingValues } from "~/settings/schema";

export async function configurePaidSettings(): Promise<void> {
  const { values } = await getStoredSettingValues();
  const updates: SettingValues = {};

  if (!values.current_stt_provider) {
    // STT is on-device only: default to the local Soniqo batch model rather
    // than any hosted/cloud model.
    updates.current_stt_provider = "hyprnote";
    updates.current_stt_model = "soniqo-parakeet-batch";
  }

  if (await needsDefaultLlmProvider(values)) {
    // No hosted provider exists anymore: default to OpenRouter with no model
    // selected, so the user configures an API key (or picks a different BYO
    // provider) themselves.
    updates.current_llm_provider = "openrouter";
  }

  await setSettingValues(updates);
}

async function needsDefaultLlmProvider(
  values: SettingValues,
): Promise<boolean> {
  const providerId = values.current_llm_provider;
  if (!providerId || !values.current_llm_model) return true;

  const provider = PROVIDERS.find((candidate) => candidate.id === providerId);
  if (!provider) return true;

  const defaultConfig = {
    base_url: provider.baseUrl || "",
    api_key: "",
  };
  if (
    getProviderSelectionBlockers(provider.requirements, {
      isAuthenticated: true,
      isPaid: true,
      config: defaultConfig,
    }).length === 0
  ) {
    return false;
  }

  let config;
  try {
    config = await getStoredAiProvider("llm", providerId);
  } catch {
    return true;
  }

  return (
    getProviderSelectionBlockers(provider.requirements, {
      isAuthenticated: true,
      isPaid: true,
      config: {
        base_url: config?.base_url || defaultConfig.base_url,
        api_key: config?.api_key || "",
      },
    }).length > 0
  );
}
