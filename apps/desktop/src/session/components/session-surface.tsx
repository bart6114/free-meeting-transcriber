import { StandardContentWrapper } from "~/shared/main";

export function SessionSurface({
  header,
  children,
}: {
  header?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <StandardContentWrapper>
      <div data-session-surface className="flex h-full flex-col">
        {header ? (
          <div data-tauri-drag-region className="px-1">
            {header}
          </div>
        ) : null}
        <div className="min-h-0 flex-1 px-2">{children}</div>
      </div>
    </StandardContentWrapper>
  );
}
