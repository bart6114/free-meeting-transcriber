import { useQuery } from "@tanstack/react-query";
import { useEffect, useRef } from "react";

import type { LocalModel } from "@hypr/plugin-local-stt";

import { NotificationProvider } from "~/contexts/notifications";
import { SttSettingsProvider, useSttSettings } from "~/settings/ai/stt/context";
import { SelectProviderAndModel } from "~/settings/ai/stt/select";
import { sttModelQueries } from "~/settings/ai/stt/shared";
import { useSetSettingValues } from "~/settings/queries";
import { useConfigValues } from "~/shared/config";
import { isFmtrLocalSttModel } from "~/stt/capabilities";

export function SttModelSection({ onContinue }: { onContinue?: () => void }) {
  return (
    <NotificationProvider>
      <SttSettingsProvider>
        <SelectProviderAndModel showAlerts={false} />
        <ContinueWhenReady onContinue={onContinue} />
      </SttSettingsProvider>
    </NotificationProvider>
  );
}

function ContinueWhenReady({ onContinue }: { onContinue?: () => void }) {
  const { queuedDownloads } = useSttSettings();
  const setSelection = useSetSettingValues();
  const { current_stt_provider, current_stt_model } = useConfigValues([
    "current_stt_provider",
    "current_stt_model",
  ] as const);
  const hasContinuedRef = useRef(false);

  const selectedLocalModel = isFmtrLocalSttModel(
    current_stt_provider,
    current_stt_model,
  )
    ? current_stt_model
    : undefined;
  const downloadedQuery = useQuery({
    ...sttModelQueries.isDownloaded(selectedLocalModel as LocalModel),
    enabled: !!selectedLocalModel,
  });
  const isSelectedModelDownloaded =
    !!selectedLocalModel && downloadedQuery.data === true;

  const queuedModel = queuedDownloads[0];

  useEffect(() => {
    if (hasContinuedRef.current) {
      return;
    }

    if (queuedModel) {
      // Persist the selection as soon as the download starts: this section
      // unmounts on advance, so nothing would select the model once the
      // background download finishes.
      hasContinuedRef.current = true;
      setSelection({
        current_stt_provider: "fmtr",
        current_stt_model: queuedModel,
      });
      onContinue?.();
      return;
    }

    if (isSelectedModelDownloaded) {
      hasContinuedRef.current = true;
      onContinue?.();
    }
  }, [queuedModel, isSelectedModelDownloaded, onContinue, setSelection]);

  return null;
}
