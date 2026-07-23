import { Icon } from "@iconify-icon/react";
import { Trans, useLingui } from "@lingui/react/macro";
import { ChevronLeft, ExternalLink, Github } from "lucide-react";
import type { ReactNode } from "react";
import { useEffect, useRef } from "react";

import { commands as openerCommands } from "@hypr/plugin-opener2";
import { OutlookIcon } from "@hypr/ui/components/icons/outlook";
import { Button } from "@hypr/ui/components/ui/button";
import { cn } from "@hypr/utils";

function useInstructionCleanup(onCleanup?: () => void) {
  const cleanupRef = useRef(onCleanup);

  useEffect(() => {
    cleanupRef.current = onCleanup;
  }, [onCleanup]);

  useEffect(() => {
    return () => {
      cleanupRef.current?.();
    };
  }, []);
}

function InstructionShell({
  title,
  description,
  icon,
  onBack,
  action,
  children,
}: {
  title: ReactNode;
  description: ReactNode;
  icon?: ReactNode;
  onBack: () => void;
  action?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="from-background via-card to-card relative flex h-full flex-col overflow-hidden bg-linear-to-b select-none">
      <div className="from-muted/40 pointer-events-none absolute inset-x-0 top-0 h-32 bg-linear-to-b to-transparent" />

      <div
        data-tauri-drag-region
        className="relative z-10 flex shrink-0 items-center px-3 pt-12"
      >
        <button
          type="button"
          onClick={onBack}
          className={cn([
            "text-muted-foreground hover:bg-muted/70 hover:text-muted-foreground flex h-9 items-center gap-1.5 rounded-full px-3 transition-colors",
          ])}
        >
          <ChevronLeft className="h-4 w-4" />
          <span className="text-xs font-medium">
            <Trans>Back</Trans>
          </span>
        </button>
      </div>

      <div
        data-tauri-drag-region
        className="relative z-10 flex flex-1 items-center justify-center p-6"
      >
        <div className="flex w-full max-w-sm flex-col items-center gap-6 px-10 pb-10 text-center">
          {icon ?? (
            <img
              src="/assets/anarlog-icon.png"
              alt=""
              className="h-14 w-14 object-contain object-center"
            />
          )}

          <div className="flex max-w-full flex-col gap-3">
            <h2 className="text-foreground font-sans text-[22px] leading-[1.15] font-semibold break-words sm:text-[28px]">
              {title}
            </h2>
            <p className="text-muted-foreground text-sm leading-6">
              {description}
            </p>
          </div>

          <div className="flex items-center gap-2.5 pt-1">
            <div className="bg-muted-foreground/75 h-1.5 w-1.5 rounded-full" />
            <div className="bg-muted h-1.5 w-1.5 rounded-full" />
            <div className="bg-muted h-1.5 w-1.5 rounded-full" />
          </div>

          {action ? <div className="w-full">{action}</div> : null}
          {children ? (
            <div className="flex w-full flex-col items-center gap-3">
              {children}
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

function ExternalInstruction({
  title,
  description,
  icon,
  actionLabel,
  onBack,
  url,
}: {
  title: string;
  description: string;
  icon?: ReactNode;
  actionLabel: string;
  onBack: () => void;
  url?: string;
}) {
  return (
    <InstructionShell
      title={title}
      description={description}
      icon={icon}
      onBack={onBack}
      action={
        url ? (
          <Button
            variant="outline"
            className={cn([
              "bg-card text-muted-foreground hover:bg-background border-border h-10 w-full",
            ])}
            onClick={() => void openerCommands.openUrl(url, null)}
          >
            {actionLabel}
            <ExternalLink className="size-3.5" />
          </Button>
        ) : undefined
      }
    />
  );
}

export function InstructionScreen({
  onBack,
  url,
  integrationId,
  onCleanup,
}: {
  onBack: () => void;
  url?: string;
  integrationId?: string;
  onCleanup?: () => void;
}) {
  const { t } = useLingui();
  useInstructionCleanup(onCleanup);

  const integration = getIntegrationInstruction(integrationId);

  return (
    <ExternalInstruction
      title={
        integration
          ? t`Connect ${integration.displayName}`
          : t`Connect your integration`
      }
      description={t`Authorize access in your browser, then return to Anarlog.`}
      icon={integration?.icon}
      actionLabel={t`Reopen in browser`}
      onBack={onBack}
      url={url}
    />
  );
}

function getIntegrationInstruction(integrationId?: string):
  | {
      displayName: string;
      icon: ReactNode;
    }
  | undefined {
  switch (integrationId) {
    case "google-calendar":
      return {
        displayName: "Google Calendar",
        icon: <Icon icon="logos:google-calendar" width={56} height={56} />,
      };
    case "outlook":
      return {
        displayName: "Outlook",
        icon: <OutlookIcon size={56} />,
      };
    case "github":
      return {
        displayName: "GitHub",
        icon: <Github className="text-foreground size-14" strokeWidth={1.5} />,
      };
    default:
      return undefined;
  }
}
