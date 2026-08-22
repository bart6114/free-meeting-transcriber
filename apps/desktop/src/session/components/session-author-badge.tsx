import { useLingui } from "@lingui/react/macro";
import { BotIcon } from "lucide-react";

import { cn } from "@hypr/utils";

import { useSession } from "~/session/queries";

/// Marks a note whose `_meta.json` carries an `author` (an agent or other
/// external writer) as not written by the vault owner. Owner-authored
/// sessions have no author and render nothing.
export function SessionAuthorBadge({
  sessionId,
  className,
}: {
  sessionId: string;
  className?: string;
}) {
  const { t } = useLingui();
  const author = useSession(sessionId)?.author ?? null;

  if (!author) {
    return null;
  }

  return (
    <div className={cn(["flex items-center", className])}>
      <span
        title={t`Written by ${author} — not by you`}
        className="bg-accent/60 text-muted-foreground inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium"
      >
        <BotIcon className="size-3" />
        {author}
      </span>
    </div>
  );
}
