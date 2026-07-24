import type { SessionMentionDropConfig } from "@hypr/editor/note";

import {
  hasSessionContextDragData,
  readSessionMentionDragData,
} from "~/shared/session-drag";

export const sessionMentionDropConfig = {
  has: hasSessionContextDragData,
  read: readSessionMentionDragData,
} satisfies SessionMentionDropConfig;
