import { useMemo } from "react";

import type { MentionConfig } from "@hypr/editor/widgets";

import { useSearchEngine } from "~/search/contexts/engine";
import { useTimelineSessionsTable } from "~/sidebar/timeline/queries";

export function useMentionConfig(): MentionConfig {
  const sessions = useTimelineSessionsTable();
  const { search } = useSearchEngine();

  return useMemo(
    () => ({
      trigger: "@",
      handleSearch: async (query: string) => {
        const results: {
          id: string;
          type: string;
          label: string;
          content?: string;
        }[] = [];

        if (query.trim()) {
          const searchResults = await search(query);
          for (const hit of searchResults) {
            results.push({
              id: hit.document.id,
              type: hit.document.type,
              label: hit.document.title,
            });
          }
        } else {
          Object.entries(sessions ?? {}).forEach(([rowId, row]) => {
            const title = row.title as string | undefined;
            if (title) {
              results.push({ id: rowId, type: "session", label: title });
            }
          });
        }

        return results.slice(0, 5);
      },
    }),
    [sessions, search],
  );
}
