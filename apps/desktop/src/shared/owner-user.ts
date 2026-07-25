import { useLiveQuery } from "~/db";
import { DEFAULT_USER_ID } from "~/shared/utils";

type OwnerUserSqlRow = {
  user_id: string;
};

const OWNER_USER_SQL = `
  SELECT owner_user_id AS user_id
  FROM sessions
  WHERE owner_user_id <> '' AND deleted_at IS NULL
  ORDER BY updated_at DESC, owner_user_id
  LIMIT 1
`;

export function useOwnerUserId(): string | null {
  const { data } = useLiveQuery<OwnerUserSqlRow, string>({
    sql: OWNER_USER_SQL,
    mapRows: (rows) => rows[0]?.user_id.trim() || DEFAULT_USER_ID,
  });
  return data ?? null;
}
