import { md2json } from "@hypr/editor/markdown";
import type { SessionEvent } from "@hypr/store";

import { WELCOME_NOTE_TRACKING_ID } from "~/onboarding/welcome-note.constants";
import { createSession } from "~/session/queries";
import { DEFAULT_USER_ID } from "~/shared/utils";
import { commands } from "~/types/tauri.gen";

const PENDING_WELCOME_SESSION_KEY = "fmtr.pending-welcome-session";

const WELCOME_NOTE = `Welcome to Free Meeting Transcriber 👋


This note is a quick way to see how Free Meeting Transcriber works.


Click **Record** in the top-right corner and say a few sentences out loud — or let any audio play on your speakers. Free Meeting Transcriber will listen, transcribe the conversation on your machine, and turn it into notes just like a real meeting.


When you stop recording, come back here to review the transcript and notes.`;

let pendingWelcomeSession: Promise<string> | null = null;

export function getOrCreateWelcomeSession(): Promise<string> {
  if (!pendingWelcomeSession) {
    pendingWelcomeSession = findOrCreateWelcomeSession().finally(() => {
      pendingWelcomeSession = null;
    });
  }
  return pendingWelcomeSession;
}

export function setPendingWelcomeSession(sessionId: string | null) {
  if (sessionId) {
    localStorage.setItem(PENDING_WELCOME_SESSION_KEY, sessionId);
  } else {
    localStorage.removeItem(PENDING_WELCOME_SESSION_KEY);
  }
}

export function takePendingWelcomeSession(): string | null {
  const sessionId = localStorage.getItem(PENDING_WELCOME_SESSION_KEY);
  localStorage.removeItem(PENDING_WELCOME_SESSION_KEY);
  return sessionId;
}

async function findOrCreateWelcomeSession(): Promise<string> {
  const result = await commands.sessionFindByTrackingId(
    WELCOME_NOTE_TRACKING_ID,
  );
  if (result.status === "error") {
    throw new Error(result.error);
  }
  if (result.data) {
    await clearStaleMeetingLink(result.data.id, result.data.event);
    return result.data.id;
  }

  const now = new Date().toISOString();
  const event: SessionEvent = {
    tracking_id: WELCOME_NOTE_TRACKING_ID,
    calendar_id: "",
    title: "Welcome to Free Meeting Transcriber",
    started_at: now,
    ended_at: "",
    is_all_day: false,
    has_recurrence_rules: false,
    meeting_link: "",
    description: "A quick introduction to Free Meeting Transcriber.",
  };

  return createSession("Welcome to Free Meeting Transcriber", DEFAULT_USER_ID, {
    event_json: JSON.stringify(event),
    raw_md: JSON.stringify(md2json(WELCOME_NOTE)),
  });
}

/**
 * Best-effort: a welcome note is still usable with a stale meeting link, so a failed clear
 * (e.g. a pre-store session with no `_meta.json` yet) logs instead of failing onboarding.
 */
async function clearStaleMeetingLink(
  sessionId: string,
  rawEvent: unknown,
): Promise<void> {
  try {
    const event = rawEvent as Partial<SessionEvent> | null | undefined;
    if (!event || typeof event !== "object" || !event.meeting_link) {
      return;
    }
    const result = await commands.sessionUpdateMeta(sessionId, {
      event: { ...event, meeting_link: "" },
    });
    if (result.status === "error") {
      console.error(
        "[welcome-note] failed to clear stale meeting link",
        result.error,
      );
    }
  } catch (error) {
    console.error("[welcome-note] failed to clear stale meeting link", error);
  }
}
