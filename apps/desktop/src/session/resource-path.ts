import { sep } from "@tauri-apps/api/path";

import { commands as fsSyncCommands } from "@hypr/plugin-fs-sync";

export async function getSessionResourcePath(
  dataDir: string,
  sessionId: string,
): Promise<string> {
  // Session folders may have human-readable names, so the physical directory
  // must come from the backend resolver; the frontend must never construct
  // session paths itself.
  try {
    const result = await fsSyncCommands.sessionDir(sessionId);
    if (result.status === "ok") {
      return result.data;
    }
  } catch {
    // fall through to the legacy layout so hooks keep receiving a path
  }
  return [dataDir, "sessions", sessionId].join(sep());
}
