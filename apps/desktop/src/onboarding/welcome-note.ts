import { md2json } from "@hypr/editor/markdown";
import type { SessionEvent } from "@hypr/store";

import { liveQueryClient } from "~/db";
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
  // The lookup itself may stay SQL until Phase E; the mutation below must not -- session meta
  // (including the event envelope) is `_meta.json`-canonical, so the stale-meeting-link clear
  // is a read-modify-write through the store command, never a raw SQL json_set.
  const rows = await liveQueryClient.execute<{
    id: string;
    event_json: string;
  }>(
    `
      SELECT id, event_json
      FROM sessions
      WHERE CASE
          WHEN json_valid(event_json)
          THEN json_extract(event_json, '$.tracking_id')
        END = ?
      ORDER BY created_at, id
      LIMIT 1
    `,
    [WELCOME_NOTE_TRACKING_ID],
  );
  if (rows[0]) {
    await clearStaleMeetingLink(rows[0].id, rows[0].event_json);
    return rows[0].id;
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
  eventJson: string,
): Promise<void> {
  try {
    const event: Partial<SessionEvent> = JSON.parse(eventJson);
    if (!event || typeof event !== "object" || !event.meeting_link) {
      return;
    }
    event.meeting_link = "";
    const result = await commands.sessionUpdateMeta(sessionId, { event });
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
