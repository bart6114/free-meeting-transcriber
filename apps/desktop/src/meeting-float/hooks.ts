import { liveQueryClient } from "~/db";
import type { RenderLabelContext } from "~/stt/live-segment";

type MeetingFloatSqlRow = {
  session_id: string;
  title: string;
  owner_user_id: string;
};

export type MeetingFloatData = {
  sessions: Record<
    string,
    {
      title: string;
      ownerUserId: string;
    }
  >;
};

const MEETING_FLOAT_SQL = `
  SELECT session.id AS session_id, session.title, session.owner_user_id
  FROM sessions AS session
  WHERE session.deleted_at IS NULL
`;

export async function loadMeetingFloatData(): Promise<MeetingFloatData> {
  return mapMeetingFloatRows(
    await liveQueryClient.execute<MeetingFloatSqlRow>(MEETING_FLOAT_SQL),
  );
}

export async function subscribeMeetingFloatData(
  onData: (data: MeetingFloatData) => void,
  onError: (error: string) => void,
): Promise<() => Promise<void>> {
  return liveQueryClient.subscribe<MeetingFloatSqlRow>(MEETING_FLOAT_SQL, [], {
    onData: (rows) => onData(mapMeetingFloatRows(rows)),
    onError,
  });
}

export function createMeetingFloatLabelContext(
  data: MeetingFloatData,
  sessionId: string,
): RenderLabelContext {
  const session = data.sessions[sessionId];
  return {
    getSelfHumanId: () => session?.ownerUserId || undefined,
    getHumanName: (speakerLabel) => speakerLabel || undefined,
    getParticipantHumanIds: () => [],
  };
}

function mapMeetingFloatRows(rows: MeetingFloatSqlRow[]): MeetingFloatData {
  const sessions: MeetingFloatData["sessions"] = {};

  for (const row of rows) {
    sessions[row.session_id] = {
      title: row.title,
      ownerUserId: row.owner_user_id,
    };
  }

  return { sessions };
}
