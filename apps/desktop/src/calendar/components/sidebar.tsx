import { useLingui } from "@lingui/react/macro";
import { platform } from "@tauri-apps/plugin-os";
import { ChevronRight } from "lucide-react";
import { useMemo } from "react";

import {
  Accordion,
  AccordionContent,
  AccordionHeader,
  AccordionItem,
  AccordionTriggerPrimitive,
} from "@hypr/ui/components/ui/accordion";
import { cn } from "@hypr/utils";

import { AppleCalendarSelection } from "./apple/calendar-selection";
import { AccessPermissionRow, TroubleShootingLink } from "./apple/permission";
import { type CalendarProvider, PROVIDERS } from "./shared";

import { usePermission } from "~/shared/hooks/usePermissions";

function getProviderBadgeClassName(badge: string) {
  if (badge === "Beta") {
    return "text-xs font-medium text-muted-foreground";
  }

  return "rounded-full border border-border px-2 text-xs font-light text-muted-foreground";
}

function ProviderIcon({ provider }: { provider: CalendarProvider }) {
  return (
    <span className="flex size-5 shrink-0 items-center justify-center">
      {provider.icon}
    </span>
  );
}

export function CalendarSidebarContent() {
  const isMacos = platform() === "macos";
  const calendar = usePermission("calendar");

  const visibleProviders = useMemo(
    () =>
      PROVIDERS.filter(
        (p) => p.platform === "all" || (p.platform === "macos" && isMacos),
      ),
    [isMacos],
  );

  return (
    <Accordion
      type="multiple"
      defaultValue={visibleProviders.map((provider) => provider.id)}
    >
      {visibleProviders.map((provider) =>
        provider.disabled ? (
          <div
            key={provider.id}
            className="-mx-2 flex items-center gap-2 px-2 py-3 opacity-50"
          >
            <ProviderIcon provider={provider} />
            <span className="text-sm font-medium">{provider.displayName}</span>
            {provider.badge && (
              <span className={getProviderBadgeClassName(provider.badge)}>
                {provider.badge}
              </span>
            )}
          </div>
        ) : (
          <ProviderAccordionItem
            key={provider.id}
            provider={provider}
            calendar={calendar}
          />
        ),
      )}
    </Accordion>
  );
}

function ProviderAccordionItem({
  provider,
  calendar,
}: {
  provider: CalendarProvider;
  calendar: ReturnType<typeof usePermission>;
}) {
  const { t } = useLingui();

  return (
    <AccordionItem value={provider.id} className="group/provider border-none">
      <div
        className={cn([
          "group/row hover:bg-accent relative -mx-2 grid grid-cols-[minmax(0,1fr)_auto] items-center gap-1 rounded-full px-2",
        ])}
      >
        <AccordionHeader className="min-w-0">
          <AccordionTriggerPrimitive className="flex w-full min-w-0 items-center py-3 text-left text-sm font-medium transition-all hover:no-underline">
            <div className="flex min-w-0 items-center gap-2">
              <ProviderIcon provider={provider} />
              <span className="flex min-w-0 items-center gap-2">
                <span className="truncate text-sm font-medium">
                  {provider.displayName}
                </span>
                {provider.badge && (
                  <span className={getProviderBadgeClassName(provider.badge)}>
                    {provider.badge}
                  </span>
                )}
              </span>
            </div>
          </AccordionTriggerPrimitive>
        </AccordionHeader>

        <ChevronRight
          className={cn([
            "text-muted-foreground size-4 shrink-0 transition-transform duration-200",
            "group-data-[state=open]/provider:rotate-90",
          ])}
        />
      </div>
      <AccordionContent className="pb-3">
        {provider.id === "apple" && (
          <div className="flex flex-col gap-3">
            {calendar.status !== "authorized" ? (
              <AccessPermissionRow
                title={t`Calendar`}
                status={calendar.status}
                isPending={calendar.isPending}
                onOpen={calendar.open}
                onRequest={calendar.request}
                onReset={calendar.reset}
              />
            ) : (
              <AppleCalendarSelection
                leftAction={
                  <TroubleShootingLink
                    isPending={calendar.isPending}
                    onOpen={calendar.open}
                    onRequest={calendar.request}
                    onReset={calendar.reset}
                  />
                }
              />
            )}
          </div>
        )}
      </AccordionContent>
    </AccordionItem>
  );
}
