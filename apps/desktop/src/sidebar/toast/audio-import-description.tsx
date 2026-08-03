import { Progress } from "@hypr/ui/components/ui/progress";

import {
  isFinishedAudioImportStatus,
  useAudioImport,
} from "~/store/zustand/audio-import";

// The sidebar toast renders its description node once (SonnerNotification only
// mounts it), so live progress has to come from a self-subscribing component.
export function AudioImportToastDescription() {
  const items = useAudioImport((state) => state.items);

  const total = items.length;
  const finished = items.filter((item) =>
    isFinishedAudioImportStatus(item.status),
  ).length;
  const current = items.find(
    (item) => !isFinishedAudioImportStatus(item.status),
  );
  const overall =
    total === 0 ? 0 : (finished + (current?.percentage ?? 0)) / total;

  return (
    <div className="flex w-full min-w-48 flex-col gap-1.5">
      <span className="truncate">
        {`Importing ${Math.min(finished + 1, total)} of ${total}`}
        {current ? ` — ${current.source.name}` : ""}
      </span>
      <Progress value={overall} />
    </div>
  );
}
