import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  GetMeetingInput,
  GetMeetingTranscriptInput as GeneratedGetMeetingTranscriptInput,
  ListMeetingsInput as GeneratedListMeetingsInput,
  Meeting,
  MeetingPage,
  SubscriptionRegistration,
  TranscriptPage,
} from "./bindings.gen";

export type {
  GetMeetingInput,
  Meeting,
  MeetingPage,
  TranscriptPage,
} from "./bindings.gen";

export type ListMeetingsInput = Partial<GeneratedListMeetingsInput>;
export type GetMeetingTranscriptInput = Pick<
  GeneratedGetMeetingTranscriptInput,
  "meeting_id"
> &
  Partial<Omit<GeneratedGetMeetingTranscriptInput, "meeting_id">>;

export type TransactionStatement = {
  sql: string;
  params: unknown[];
  expectedRowsAffected?: number;
};

export type QueryEvent<T = Record<string, unknown>> =
  | { event: "result"; data: T[] }
  | { event: "error"; data: string };

export async function listMeetings(
  input: ListMeetingsInput,
): Promise<MeetingPage> {
  return invoke("plugin:db|list_meetings", { input });
}

export async function getMeeting(input: GetMeetingInput): Promise<Meeting> {
  return invoke("plugin:db|get_meeting", { input });
}

export async function getMeetingTranscript(
  input: GetMeetingTranscriptInput,
): Promise<TranscriptPage> {
  return invoke("plugin:db|get_meeting_transcript", { input });
}

// Generic query path: returns named object rows for app-level SQL consumers.
export async function execute<T = Record<string, unknown>>(
  sql: string,
  params: unknown[] = [],
): Promise<T[]> {
  return invoke("plugin:db|execute", { sql, params });
}

export async function executeTransaction(
  statements: TransactionStatement[],
): Promise<number[]> {
  return invoke("plugin:db|execute_transaction", { statements });
}

// Drizzle proxy path: returns raw positional rows in sqlite-proxy format.
export async function executeProxy(
  sql: string,
  params: unknown[] = [],
  method: "run" | "all" | "get" | "values",
): Promise<{ rows: unknown[] }> {
  return invoke("plugin:db|execute_proxy", { sql, params, method });
}

export async function subscribe<T = Record<string, unknown>>(
  sql: string,
  params: unknown[],
  options: {
    onData: (rows: T[]) => void;
    onError?: (error: string) => void;
  },
): Promise<() => Promise<void>> {
  const channel = new Channel<QueryEvent<T>>();

  channel.onmessage = (event) => {
    if (event.event === "result") {
      options.onData(event.data);
      return;
    }

    options.onError?.(event.data);
  };

  const registration: SubscriptionRegistration = await invoke(
    "plugin:db|subscribe",
    {
      sql,
      params,
      onEvent: channel,
    },
  );

  if (registration.analysis.kind === "non_reactive") {
    console.warn(
      `[plugin-db] live query subscription is non-reactive for SQL "${sql}": ${registration.analysis.data.reason}`,
    );
  }

  return async () => {
    channel.onmessage = () => {};
    await invoke("plugin:db|unsubscribe", { subscriptionId: registration.id });
  };
}
