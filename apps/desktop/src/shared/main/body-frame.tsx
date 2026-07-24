import { MainBodyPanel } from "./body-panel";
import {
  MainSessionStatusBannerHost,
  SessionStatusBannerProvider,
} from "./session-status-banner";

export function MainShellBodyFrame({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <SessionStatusBannerProvider>
      <MainBodyPanel>{children}</MainBodyPanel>
      <MainSessionStatusBannerHost />
    </SessionStatusBannerProvider>
  );
}
