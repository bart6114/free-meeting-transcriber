import { AudioLinesIcon, PaperclipIcon } from "lucide-react";

import { cn } from "@hypr/utils";

import type { NoteFileDragKind } from "./file-handler";

import { AUDIO_EXTENSIONS } from "~/stt/useUploadFile";

const supportedAudioFormats = formatAudioExtensionList(AUDIO_EXTENSIONS);

export function FileDropTarget({ kind }: { kind: NoteFileDragKind | null }) {
  if (!kind) {
    return null;
  }

  const isAudio = kind === "audio";
  const Icon = isAudio ? AudioLinesIcon : PaperclipIcon;
  const description =
    kind === "mixed"
      ? "Audio will be transcribed; other files will be attached."
      : "Images appear inline; other files are added as attachments.";

  return (
    <div
      role="status"
      aria-live="polite"
      className={cn([
        "pointer-events-none absolute inset-0 z-30 flex items-center justify-center rounded-lg border border-dashed",
        "border-border/70 bg-background/30 text-muted-foreground shadow-inner",
        "[background-image:radial-gradient(circle_at_center,_rgba(113,113,122,0.16)_1px,_transparent_1px)]",
        "[background-size:18px_18px]",
      ])}
    >
      <div className="border-border/70 bg-card/95 text-foreground flex items-center gap-3 rounded-md border px-4 py-3 shadow-sm">
        <Icon className="text-muted-foreground size-5 shrink-0" />
        <div className="flex min-w-0 flex-col gap-0.5">
          <p className="text-sm font-medium">
            {isAudio
              ? "Drop to upload and transcribe audio"
              : "Drop files here to attach to note"}
          </p>
          <p className="text-muted-foreground text-xs">
            {isAudio ? `${supportedAudioFormats} audio` : description}
          </p>
        </div>
      </div>
    </div>
  );
}

function formatAudioExtensionList(extensions: string[]) {
  const labels = extensions.map((extension) => extension.toUpperCase());
  if (labels.length <= 1) {
    return labels.join("");
  }

  return `${labels.slice(0, -1).join(", ")}, or ${labels[labels.length - 1]}`;
}
