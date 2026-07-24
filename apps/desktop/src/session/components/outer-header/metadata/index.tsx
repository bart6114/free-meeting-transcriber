import { CalendarIcon } from "lucide-react";
import { forwardRef, type ReactElement, useState } from "react";

import { Button } from "@hypr/ui/components/ui/button";
import {
  AppFloatingPanel,
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@hypr/ui/components/ui/popover";
import { cn } from "@hypr/utils";

import { DateEditor } from "./date";

export function MetadataButton({
  sessionId,
  renderTrigger,
}: {
  sessionId: string;
  renderTrigger?: (props: { open: boolean; label: string }) => ReactElement;
}) {
  const [open, setOpen] = useState(false);
  const label = "Open note metadata";

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        {renderTrigger ? (
          renderTrigger({ open, label })
        ) : (
          <TriggerInner label={label} open={open} />
        )}
      </PopoverTrigger>
      <PopoverContent
        variant="app"
        align="end"
        className="w-85 overflow-hidden"
      >
        <AppFloatingPanel className="scrollbar-soft max-h-[80vh] min-h-0 overflow-x-hidden overflow-y-auto overscroll-contain">
          <ContentInner sessionId={sessionId} />
        </AppFloatingPanel>
      </PopoverContent>
    </Popover>
  );
}

const TriggerInner = forwardRef<
  HTMLButtonElement,
  { label: string; open?: boolean }
>(({ label, open, ...props }, ref) => {
  return (
    <Button
      ref={ref}
      {...props}
      variant="ghost"
      size="icon"
      type="button"
      data-tauri-drag-region="false"
      aria-label={label}
      title={label}
      className={cn([
        "size-7 rounded-full",
        "text-muted-foreground hover:bg-accent hover:text-foreground",
        open && "bg-muted text-foreground",
      ])}
    >
      <CalendarIcon size={16} />
    </Button>
  );
});

function ContentInner({ sessionId }: { sessionId: string }) {
  return (
    <div className="flex flex-col gap-4 p-4">
      <DateEditor sessionId={sessionId} />
    </div>
  );
}
