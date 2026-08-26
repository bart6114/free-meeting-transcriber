import { md2json } from "@hypr/editor/markdown";

import { WELCOME_NOTE_TRACKING_ID } from "~/onboarding/welcome-note.constants";
import { createSession } from "~/session/queries";
import { DEFAULT_USER_ID } from "~/shared/utils";
import { commands } from "~/types/tauri.gen";

const PENDING_WELCOME_SESSION_KEY = "loofah.pending-welcome-session";
const LEGACY_PENDING_WELCOME_SESSION_KEY = "fmtr.pending-welcome-session";

const WELCOME_NOTE = `Welcome to Loofah 👋


This note is a quick way to see how Loofah works.


Click **Record** in the top-right corner and say a few sentences out loud — or let any audio play on your speakers. Loofah will listen, transcribe the conversation on your machine, and turn it into notes just like a real meeting.


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
    localStorage.removeItem(LEGACY_PENDING_WELCOME_SESSION_KEY);
  }
}

export function takePendingWelcomeSession(): string | null {
  const sessionId =
    localStorage.getItem(PENDING_WELCOME_SESSION_KEY) ??
    localStorage.getItem(LEGACY_PENDING_WELCOME_SESSION_KEY);
  localStorage.removeItem(PENDING_WELCOME_SESSION_KEY);
  localStorage.removeItem(LEGACY_PENDING_WELCOME_SESSION_KEY);
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
    return result.data.id;
  }

  return createSession("Welcome to Loofah", DEFAULT_USER_ID, {
    tracking_id: WELCOME_NOTE_TRACKING_ID,
    raw_md: JSON.stringify(md2json(WELCOME_NOTE)),
  });
}
