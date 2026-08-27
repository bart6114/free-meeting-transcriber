import { commands } from "~/types/tauri.gen";

export async function queueTagSuggestions(sessionId: string): Promise<void> {
  const result = await commands.sessionQueueTagSuggestions(sessionId);
  if (result.status === "error") {
    console.error("[related-tags] failed to queue suggestions", result.error);
  }
}
