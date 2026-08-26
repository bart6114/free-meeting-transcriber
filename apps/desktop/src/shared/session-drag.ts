const SESSION_CONTEXT_DRAG_TYPE = "application/x-loofah-session-context";
const LEGACY_SESSION_CONTEXT_DRAG_TYPE = "application/x-fmtr-session-context";

type SessionDragPayload = {
  sessionId: string;
  title?: string;
};

type SessionMentionDragData = {
  id: string;
  label: string;
};

export const hasSessionContextDragData = (
  dataTransfer: Pick<DataTransfer, "types"> | null | undefined,
) => {
  if (!dataTransfer) {
    return false;
  }

  return [SESSION_CONTEXT_DRAG_TYPE, LEGACY_SESSION_CONTEXT_DRAG_TYPE].some(
    (type) => Array.from(dataTransfer.types).includes(type),
  );
};

export const writeSessionContextDragData = (
  dataTransfer: DataTransfer,
  sessionId: string,
  fallbackText: string,
) => {
  const title = fallbackText.trim() || "Untitled";

  dataTransfer.effectAllowed = "copy";
  dataTransfer.setData(
    SESSION_CONTEXT_DRAG_TYPE,
    JSON.stringify({ sessionId, title }),
  );
  dataTransfer.setData("text/plain", title);
};

const readSessionContextDragPayload = (
  dataTransfer: Pick<DataTransfer, "getData" | "types"> | null | undefined,
): SessionDragPayload | null => {
  if (!dataTransfer || !hasSessionContextDragData(dataTransfer)) {
    return null;
  }

  try {
    const payload = JSON.parse(
      dataTransfer.getData(SESSION_CONTEXT_DRAG_TYPE) ||
        dataTransfer.getData(LEGACY_SESSION_CONTEXT_DRAG_TYPE),
    ) as SessionDragPayload;

    if (
      typeof payload.sessionId !== "string" ||
      payload.sessionId.trim().length === 0
    ) {
      return null;
    }

    return {
      sessionId: payload.sessionId,
      title:
        typeof payload.title === "string" && payload.title.trim().length > 0
          ? payload.title.trim()
          : undefined,
    };
  } catch {
    return null;
  }
};

export const readSessionMentionDragData = (
  dataTransfer: Pick<DataTransfer, "getData" | "types"> | null | undefined,
): SessionMentionDragData | null => {
  const payload = readSessionContextDragPayload(dataTransfer);
  if (!payload) {
    return null;
  }

  return {
    id: payload.sessionId,
    label: payload.title ?? "Untitled",
  };
};
