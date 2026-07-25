use sqlx::{QueryBuilder, Sqlite, SqlitePool};

use crate::{
    ListSessions, SessionActionItemRow, SessionDocumentRow, SessionListItem, SessionRow,
    SessionTranscriptRow,
};

pub const MAX_SESSION_LIST_LIMIT: u32 = 500;

const SESSION_LIST_COLUMNS: &str = "
    SELECT id, title, kind, status, created_at, updated_at, started_at, ended_at
    FROM sessions
";

const SESSION_COLUMNS: &str = "
    SELECT id, owner_user_id, title, kind, status, created_at, updated_at,
           started_at, ended_at, timezone, language, external_provider, source_apps_json,
           folder_path, slug, metadata_json
    FROM sessions
";

const SESSION_DOCUMENT_COLUMNS: &str = "
    SELECT document.id, document.session_id, document.kind,
           document.template_id, document.title, document.body_format, document.body,
           document.source_hash, document.generation_metadata_json, document.sort_order,
           document.created_by, document.updated_by, document.created_at, document.updated_at
    FROM session_documents AS document
    JOIN sessions AS session ON session.id = document.session_id AND session.deleted_at IS NULL
";

const SESSION_TRANSCRIPT_COLUMNS: &str = "
    SELECT transcript.id, transcript.owner_user_id,
           transcript.session_id, transcript.source, transcript.provider, transcript.model,
           transcript.language, transcript.started_at_ms, transcript.ended_at_ms,
           transcript.audio_attachment_id, transcript.memo, transcript.words_json,
           transcript.speaker_hints_json, transcript.metadata_json, transcript.created_at,
           transcript.updated_at
    FROM transcripts AS transcript
    JOIN sessions AS session ON session.id = transcript.session_id AND session.deleted_at IS NULL
";

const SESSION_ACTION_ITEM_COLUMNS: &str = "
    SELECT action_item.id, action_item.session_id,
           action_item.source_type, action_item.source_id, action_item.source_order,
           action_item.assignee_human_id, action_item.status, action_item.text,
           action_item.body_json, action_item.due_at, action_item.completed_at,
           action_item.created_by, action_item.updated_by, action_item.metadata_json,
           action_item.created_at, action_item.updated_at
    FROM action_items AS action_item
    JOIN sessions AS session ON session.id = action_item.session_id AND session.deleted_at IS NULL
";

pub async fn list_sessions(
    pool: &SqlitePool,
    input: ListSessions<'_>,
) -> Result<Vec<SessionListItem>, sqlx::Error> {
    let mut query = QueryBuilder::<Sqlite>::new(SESSION_LIST_COLUMNS);
    query.push(" WHERE deleted_at IS NULL");

    if let Some(search) = input.query.map(str::trim).filter(|query| !query.is_empty()) {
        query.push(" AND (instr(lower(title), lower(");
        query.push_bind(search);
        query.push(")) > 0 OR instr(lower(id), lower(");
        query.push_bind(search);
        query.push(")) > 0)");
    }

    query.push(
        " ORDER BY COALESCE(NULLIF(started_at, ''), created_at) DESC, created_at DESC, id DESC",
    );
    query.push(" LIMIT ");
    query.push_bind(i64::from(input.limit.min(MAX_SESSION_LIST_LIMIT)));
    query.push(" OFFSET ");
    query.push_bind(i64::from(input.offset));

    query
        .build_query_as::<SessionListItem>()
        .fetch_all(pool)
        .await
}

pub async fn get_session(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<Option<SessionRow>, sqlx::Error> {
    let mut query = QueryBuilder::<Sqlite>::new(SESSION_COLUMNS);
    query.push(" WHERE id = ");
    query.push_bind(session_id);
    query.push(" AND deleted_at IS NULL LIMIT 1");
    query
        .build_query_as::<SessionRow>()
        .fetch_optional(pool)
        .await
}

pub async fn list_session_documents(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<Vec<SessionDocumentRow>, sqlx::Error> {
    let mut query = QueryBuilder::<Sqlite>::new(SESSION_DOCUMENT_COLUMNS);
    query.push(" WHERE document.session_id = ");
    query.push_bind(session_id);
    query.push(
        " AND document.deleted_at IS NULL
          ORDER BY document.sort_order, document.created_at, document.id",
    );
    query
        .build_query_as::<SessionDocumentRow>()
        .fetch_all(pool)
        .await
}

pub async fn get_session_note(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<Option<SessionDocumentRow>, sqlx::Error> {
    let mut query = QueryBuilder::<Sqlite>::new(SESSION_DOCUMENT_COLUMNS);
    query.push(" WHERE document.session_id = ");
    query.push_bind(session_id);
    query.push(
        " AND document.kind = 'note' AND document.deleted_at IS NULL
          ORDER BY CASE WHEN document.id = ",
    );
    query.push_bind(session_id);
    query.push(" THEN 0 ELSE 1 END, document.created_at, document.id LIMIT 1");
    query
        .build_query_as::<SessionDocumentRow>()
        .fetch_optional(pool)
        .await
}

pub async fn list_session_transcripts(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<Vec<SessionTranscriptRow>, sqlx::Error> {
    let mut query = QueryBuilder::<Sqlite>::new(SESSION_TRANSCRIPT_COLUMNS);
    query.push(" WHERE transcript.session_id = ");
    query.push_bind(session_id);
    query.push(
        " AND transcript.deleted_at IS NULL
          ORDER BY transcript.started_at_ms, transcript.created_at, transcript.id",
    );
    query
        .build_query_as::<SessionTranscriptRow>()
        .fetch_all(pool)
        .await
}

pub async fn list_session_action_items(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<Vec<SessionActionItemRow>, sqlx::Error> {
    let mut query = QueryBuilder::<Sqlite>::new(SESSION_ACTION_ITEM_COLUMNS);
    query.push(" WHERE action_item.session_id = ");
    query.push_bind(session_id);
    query.push(
        " AND action_item.deleted_at IS NULL
          ORDER BY action_item.source_order, action_item.created_at, action_item.id",
    );
    query
        .build_query_as::<SessionActionItemRow>()
        .fetch_all(pool)
        .await
}

#[cfg(test)]
mod tests {
    use hypr_db_core::Db;

    use super::*;

    async fn test_db() -> Db {
        let db = Db::connect_memory_plain().await.unwrap();
        crate::prepare_schema(&db).await.unwrap();
        db
    }

    async fn insert_session(pool: &SqlitePool, id: &str, title: &str, started_at: &str) {
        sqlx::query(
            "INSERT INTO sessions
             (id, title, started_at, source_apps_json, metadata_json)
             VALUES (?, ?, ?, '[{\"app\":\"zoom\"}]', '{\"source\":\"test\"}')",
        )
        .bind(id)
        .bind(title)
        .bind(started_at)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn list_sessions_filters_searches_and_paginates_deterministically() {
        let db = test_db().await;
        insert_session(db.pool(), "alpha-old", "Alpha Planning", "2026-01-01").await;
        insert_session(db.pool(), "alpha-new", "ALPHA Review", "2026-02-01").await;
        insert_session(db.pool(), "beta", "Beta Review", "2026-03-01").await;
        sqlx::query("UPDATE sessions SET deleted_at = '2026-04-01' WHERE id = 'beta'")
            .execute(db.pool())
            .await
            .unwrap();

        let first = list_sessions(
            db.pool(),
            ListSessions {
                query: Some(" alpha "),
                limit: 1,
                offset: 0,
            },
        )
        .await
        .unwrap();
        let second = list_sessions(
            db.pool(),
            ListSessions {
                query: Some("alpha"),
                limit: 1,
                offset: 1,
            },
        )
        .await
        .unwrap();
        let by_id = list_sessions(
            db.pool(),
            ListSessions {
                query: Some("old"),
                limit: 10,
                offset: 0,
            },
        )
        .await
        .unwrap();

        assert_eq!(first[0].id, "alpha-new");
        assert_eq!(second[0].id, "alpha-old");
        assert_eq!(by_id[0].id, "alpha-old");
    }

    #[tokio::test]
    async fn session_read_helpers_return_active_rows_and_preserve_opaque_json() {
        let db = test_db().await;
        insert_session(db.pool(), "session-1", "Planning", "2026-01-01").await;
        sqlx::query(
            "INSERT INTO session_documents
             (id, session_id, kind, body, sort_order, created_at)
             VALUES
             ('fallback', 'session-1', 'note', 'fallback body', 0, '2026-01-01'),
             ('session-1', 'session-1', 'note', 'canonical body', 1, '2026-02-01'),
             ('summary', 'session-1', 'summary', 'summary body', 2, '2026-03-01'),
             ('deleted', 'session-1', 'note', 'deleted body', 3, '2026-04-01')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query("UPDATE session_documents SET deleted_at = '2026-05-01' WHERE id = 'deleted'")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO transcripts
             (id, session_id, started_at_ms, words_json, speaker_hints_json)
             VALUES ('later', 'session-1', 200, '[{\"word\":\"later\"}]', '[]'),
                    ('earlier', 'session-1', 100, '[{\"word\":\"earlier\"}]', '[]')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO action_items (id, session_id, source_order, text, body_json)
             VALUES ('action-2', 'session-1', 2, 'Second', '{\"rank\":2}'),
                    ('action-1', 'session-1', 1, 'First', '{\"rank\":1}')",
        )
        .execute(db.pool())
        .await
        .unwrap();

        let session = get_session(db.pool(), "session-1").await.unwrap().unwrap();
        let documents = list_session_documents(db.pool(), "session-1")
            .await
            .unwrap();
        let note = get_session_note(db.pool(), "session-1")
            .await
            .unwrap()
            .unwrap();
        let transcripts = list_session_transcripts(db.pool(), "session-1")
            .await
            .unwrap();
        let action_items = list_session_action_items(db.pool(), "session-1")
            .await
            .unwrap();

        assert_eq!(session.source_apps_json, "[{\"app\":\"zoom\"}]");
        assert_eq!(
            documents
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            vec!["fallback", "session-1", "summary"]
        );
        assert_eq!(note.id, "session-1");
        assert_eq!(
            transcripts
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            vec!["earlier", "later"]
        );
        assert_eq!(
            action_items
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            vec!["action-1", "action-2"]
        );
        assert_eq!(action_items[0].body_json, "{\"rank\":1}");

        sqlx::query(
            "UPDATE session_documents SET deleted_at = '2026-06-01' WHERE id = 'session-1'",
        )
        .execute(db.pool())
        .await
        .unwrap();
        let fallback = get_session_note(db.pool(), "session-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fallback.id, "fallback");

        sqlx::query("UPDATE sessions SET deleted_at = '2026-07-01' WHERE id = 'session-1'")
            .execute(db.pool())
            .await
            .unwrap();
        assert!(
            list_session_documents(db.pool(), "session-1")
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            list_session_transcripts(db.pool(), "session-1")
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            list_session_action_items(db.pool(), "session-1")
                .await
                .unwrap()
                .is_empty()
        );
    }
}
