use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::{SessionStore, StoreError, paths};

pub const ENHANCED_KINDS: [&str; 2] = ["summary", "template_output"];

/// One AI-generated document (`summary` or `template_output`), file-canonical at
/// `sessions/<session_id>/enhanced/<id>.md`. `id` is the same UUID the `session_documents`
/// index row uses, and the frontmatter carries every metadata column that row mirrors --
/// there is deliberately no sidecar file.
#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct EnhancedDoc {
    pub id: String,
    pub session_id: String,
    /// "summary" (no template) or "template_output".
    pub kind: String,
    pub title: String,
    pub template_id: String,
    pub sort_order: i32,
    /// Body only -- never includes the frontmatter block.
    pub markdown: String,
}

/// Partial update for an existing enhanced doc: `None` means "leave as-is". The `expected_*`
/// fields are compare-and-swap guards against the *current file content* -- a mismatch
/// returns `StoreError::Conflict` and changes nothing, which is the store-level equivalent
/// of the SQL era's `expectedRowsAffected`/`WHERE title = ?` rejections.
#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, Default, PartialEq)]
pub struct EnhancedDocPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_markdown: Option<String>,
}

/// The typed frontmatter schema for `enhanced/<id>.md`. Every field defaults so a
/// hand-created or partially-written file still parses (missing kind falls back to
/// "summary" below); unknown keys are ignored by serde. `hypr_frontmatter::Document`'s
/// renderer sorts keys, so the on-disk bytes are deterministic for identical content.
#[derive(Serialize, Deserialize, Debug)]
struct EnhancedFrontmatter {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    template_id: String,
    #[serde(default)]
    sort_order: i32,
}

pub(super) fn render_enhanced_file(doc: &EnhancedDoc) -> Result<String, StoreError> {
    let frontmatter = EnhancedFrontmatter {
        kind: doc.kind.clone(),
        title: doc.title.clone(),
        template_id: doc.template_id.clone(),
        sort_order: doc.sort_order,
    };
    hypr_frontmatter::Document::new(frontmatter, &doc.markdown)
        .render()
        .map_err(|e| StoreError::Serialize(format!("failed to render enhanced doc: {e}")))
}

/// Inverse of `render_enhanced_file`. A file with no frontmatter at all (an external
/// drop-in) is accepted as a plain summary; a file whose frontmatter is present but
/// malformed is an error -- rebuild treats that like any other unparseable artifact
/// (log, leave the existing index row alone). An unknown `kind` degrades to "summary"
/// rather than erroring so a hand-edited value can't make the document vanish.
pub(super) fn parse_enhanced_file(
    id: &str,
    session_id: &str,
    raw: &str,
) -> Result<EnhancedDoc, StoreError> {
    let (frontmatter, markdown) =
        match hypr_frontmatter::Document::<EnhancedFrontmatter>::from_str(raw) {
            Ok(doc) => (doc.frontmatter, doc.content),
            Err(hypr_frontmatter::Error::MissingOpeningDelimiter) => (
                EnhancedFrontmatter {
                    kind: String::new(),
                    title: String::new(),
                    template_id: String::new(),
                    sort_order: 0,
                },
                raw.to_string(),
            ),
            Err(e) => {
                return Err(StoreError::Serialize(format!(
                    "failed to parse enhanced doc frontmatter: {e}"
                )));
            }
        };

    let kind = if ENHANCED_KINDS.contains(&frontmatter.kind.as_str()) {
        frontmatter.kind
    } else {
        "summary".to_string()
    };

    Ok(EnhancedDoc {
        id: id.to_string(),
        session_id: session_id.to_string(),
        kind,
        title: frontmatter.title,
        template_id: frontmatter.template_id,
        sort_order: frontmatter.sort_order,
        markdown,
    })
}

impl SessionStore {
    /// Create-or-replace an enhanced doc: file write first, then the SQL dual-write
    /// (frontend reads and search stay on `session_documents` until Phases E/F). Requires
    /// the session's `_meta.json` to exist -- creating a doc must never resurrect a
    /// session folder that a racing delete just trashed.
    pub async fn write_enhanced_doc(&self, doc: &EnhancedDoc) -> Result<(), StoreError> {
        validate_kind(&doc.kind)?;

        if self.read_meta(&doc.session_id).await?.is_none() {
            return Err(StoreError::Io(format!(
                "session {} has no _meta.json; refusing to create an enhanced doc",
                doc.session_id
            )));
        }

        self.persist_enhanced_doc(doc).await
    }

    /// Read-modify-write partial update. Errors when the doc file doesn't exist (the SQL
    /// era's `expectedRowsAffected: 1` "row vanished" rejection); `Conflict` when a CAS
    /// guard in the patch doesn't match the current file content.
    pub async fn update_enhanced_doc(
        &self,
        session_id: &str,
        doc_id: &str,
        patch: EnhancedDocPatch,
    ) -> Result<(), StoreError> {
        let mut doc = self
            .read_enhanced_doc(session_id, doc_id)
            .await?
            .ok_or_else(|| {
                StoreError::Io(format!(
                    "enhanced doc {doc_id} in session {session_id} has no file to update"
                ))
            })?;

        if let Some(expected) = &patch.expected_title {
            if &doc.title != expected {
                return Err(StoreError::Conflict(format!(
                    "enhanced doc {doc_id} title changed (expected {expected:?}, found {:?})",
                    doc.title
                )));
            }
        }
        if let Some(expected) = &patch.expected_markdown {
            if &doc.markdown != expected {
                return Err(StoreError::Conflict(format!(
                    "enhanced doc {doc_id} body changed since it was read"
                )));
            }
        }

        let EnhancedDocPatch {
            kind,
            title,
            template_id,
            sort_order,
            markdown,
            expected_title: _,
            expected_markdown: _,
        } = patch;

        if let Some(kind) = kind {
            validate_kind(&kind)?;
            doc.kind = kind;
        }
        if let Some(title) = title {
            doc.title = title;
        }
        if let Some(template_id) = template_id {
            doc.template_id = template_id;
        }
        if let Some(sort_order) = sort_order {
            doc.sort_order = sort_order;
        }
        if let Some(markdown) = markdown {
            doc.markdown = markdown;
        }

        self.persist_enhanced_doc(&doc).await
    }

    pub async fn read_enhanced_doc(
        &self,
        session_id: &str,
        doc_id: &str,
    ) -> Result<Option<EnhancedDoc>, StoreError> {
        let vault_base = self.vault_base.clone();
        let session_id = session_id.to_string();
        let doc_id = doc_id.to_string();

        tokio::task::spawn_blocking(move || -> Result<Option<EnhancedDoc>, StoreError> {
            let path = vault_base.join(paths::enhanced_doc_path(&session_id, &doc_id));
            // Attempt-then-match, same rationale as read_meta: never mistake a transient
            // read failure for "doc doesn't exist".
            let raw = match std::fs::read_to_string(&path) {
                Ok(raw) => raw,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => {
                    return Err(StoreError::Io(format!(
                        "failed to read enhanced doc file: {e}"
                    )));
                }
            };
            parse_enhanced_file(&doc_id, &session_id, &raw).map(Some)
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {e}")))?
    }

    /// File to `.trash/<date>/sessions/<id>/enhanced/<doc>.md` (hand-recoverable, never
    /// synced), then a hard `DELETE` of the index row. No tombstone: no frontend undo path
    /// exists for enhanced notes, and rebuild prunes file-less rows anyway -- a
    /// `deleted_at` marker would only survive until the next startup/focus rescan.
    /// Idempotent: deleting a doc that doesn't exist succeeds.
    pub async fn delete_enhanced_doc(
        &self,
        session_id: &str,
        doc_id: &str,
    ) -> Result<(), StoreError> {
        let vault_base = self.vault_base.clone();
        let relative = paths::enhanced_doc_path(session_id, doc_id);

        tokio::task::spawn_blocking(move || -> Result<(), StoreError> {
            let abs = vault_base.join(relative);
            hypr_fs_sync_core::export::move_to_trash(&vault_base, &abs).map_err(|e| {
                StoreError::Io(format!("failed to move enhanced doc to trash: {e}"))
            })?;
            Ok(())
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {e}")))??;

        sqlx::query("DELETE FROM session_documents WHERE id = ? AND session_id = ?")
            .bind(doc_id)
            .bind(session_id)
            .execute(self.pool())
            .await
            .map_err(|e| StoreError::Db(e.to_string()))?;

        Ok(())
    }

    async fn persist_enhanced_doc(&self, doc: &EnhancedDoc) -> Result<(), StoreError> {
        let rendered = render_enhanced_file(doc)?;
        self.write_file(
            paths::enhanced_doc_path(&doc.session_id, &doc.id),
            rendered.into_bytes(),
        )
        .await?;
        self.upsert_enhanced_doc_row(doc).await
    }

    /// Shared by the interactive write path and rebuild. The `WHERE` guard keeps an
    /// unchanged doc from re-firing `session_documents`' `search_index_dirty` trigger on
    /// every automatic rebuild/refresh pass (same rationale as `upsert_document_row`).
    /// `deleted_at = NULL` on a genuine change: a present file is a live document by
    /// definition (mirror honesty), and the delete path hard-deletes rows anyway.
    pub(super) async fn upsert_enhanced_doc_row(
        &self,
        doc: &EnhancedDoc,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO session_documents (id, session_id, kind, template_id, title, sort_order, body_format, body, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, 'md', ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(id) DO UPDATE SET
               session_id = excluded.session_id,
               kind = excluded.kind,
               template_id = excluded.template_id,
               title = excluded.title,
               sort_order = excluded.sort_order,
               body_format = excluded.body_format,
               body = excluded.body,
               deleted_at = NULL
             WHERE session_id IS NOT excluded.session_id
                OR kind IS NOT excluded.kind
                OR template_id IS NOT excluded.template_id
                OR title IS NOT excluded.title
                OR sort_order IS NOT excluded.sort_order
                OR body_format IS NOT excluded.body_format
                OR body IS NOT excluded.body
                OR deleted_at IS NOT NULL",
        )
        .bind(&doc.id)
        .bind(&doc.session_id)
        .bind(&doc.kind)
        .bind(&doc.template_id)
        .bind(&doc.title)
        .bind(doc.sort_order)
        .bind(&doc.markdown)
        .execute(self.pool())
        .await
        .map_err(|e| StoreError::Db(e.to_string()))?;
        Ok(())
    }
}

fn validate_kind(kind: &str) -> Result<(), StoreError> {
    if ENHANCED_KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(StoreError::Serialize(format!(
            "invalid enhanced doc kind {kind:?} (expected one of {ENHANCED_KINDS:?})"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::super::content::SessionMeta;
    use super::*;

    fn meta(id: &str) -> SessionMeta {
        SessionMeta {
            id: id.to_string(),
            title: "Session".to_string(),
            started_at: None,
            ended_at: None,
            created_at: "2026-07-24T00:00:00Z".to_string(),
            tags: vec![],
            event: None,
            folder: None,
        }
    }

    fn doc(session_id: &str, doc_id: &str) -> EnhancedDoc {
        EnhancedDoc {
            id: doc_id.to_string(),
            session_id: session_id.to_string(),
            kind: "template_output".to_string(),
            title: "Customer review".to_string(),
            template_id: "template-1".to_string(),
            sort_order: 3,
            markdown: "# Review\n\n- Point".to_string(),
        }
    }

    async fn test_store() -> (SessionStore, tempfile::TempDir) {
        let temp = tempfile::tempdir().unwrap();
        let vault = temp.path().to_path_buf();
        let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
        hypr_db_app::prepare_schema(&db).await.unwrap();
        let store = SessionStore::new(vault, db.pool().clone());
        (store, temp)
    }

    #[tokio::test]
    async fn write_enhanced_doc_round_trips_file_and_index() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1")).await.unwrap();
        let d = doc("s1", "doc-1");
        store.write_enhanced_doc(&d).await.unwrap();

        assert!(vault.path().join("sessions/s1/enhanced/doc-1.md").is_file());
        assert_eq!(
            store.read_enhanced_doc("s1", "doc-1").await.unwrap(),
            Some(d.clone())
        );

        let (kind, template_id, title, sort_order, body_format, body): (
            String,
            String,
            String,
            i64,
            String,
            String,
        ) = sqlx::query_as(
            "SELECT kind, template_id, title, sort_order, body_format, body
             FROM session_documents WHERE id='doc-1' AND session_id='s1'",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(kind, "template_output");
        assert_eq!(template_id, "template-1");
        assert_eq!(title, "Customer review");
        assert_eq!(sort_order, 3);
        assert_eq!(body_format, "md");
        assert_eq!(body, "# Review\n\n- Point");
    }

    /// The file itself carries the metadata (frontmatter, no sidecar) and only the body
    /// below the closing delimiter -- the delimiter discipline is what lets rebuild restore
    /// title/template_id/sort_order/kind from the file alone.
    #[tokio::test]
    async fn enhanced_file_has_frontmatter_metadata_and_bare_body() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1")).await.unwrap();
        store.write_enhanced_doc(&doc("s1", "doc-1")).await.unwrap();

        let raw =
            std::fs::read_to_string(vault.path().join("sessions/s1/enhanced/doc-1.md")).unwrap();
        assert!(raw.starts_with("---\n"));
        assert!(raw.contains("title: Customer review"));
        assert!(raw.contains("template_id: template-1"));
        assert!(raw.contains("sort_order: 3"));
        assert!(raw.contains("kind: template_output"));
        assert!(raw.ends_with("# Review\n\n- Point"));
    }

    #[tokio::test]
    async fn write_enhanced_doc_refuses_a_session_without_meta() {
        let (store, vault) = test_store().await;
        let result = store.write_enhanced_doc(&doc("ghost", "doc-1")).await;
        assert!(result.is_err());
        assert!(!vault.path().join("sessions/ghost").exists());
    }

    #[tokio::test]
    async fn write_enhanced_doc_rejects_an_unknown_kind() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1")).await.unwrap();
        let mut d = doc("s1", "doc-1");
        d.kind = "note".to_string();
        assert!(store.write_enhanced_doc(&d).await.is_err());
    }

    #[tokio::test]
    async fn update_enhanced_doc_patches_only_the_given_fields() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1")).await.unwrap();
        let d = doc("s1", "doc-1");
        store.write_enhanced_doc(&d).await.unwrap();

        store
            .update_enhanced_doc(
                "s1",
                "doc-1",
                EnhancedDocPatch {
                    markdown: Some("# Replaced".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let after = store
            .read_enhanced_doc("s1", "doc-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.markdown, "# Replaced");
        assert_eq!(after.title, d.title, "unpatched fields must survive");
        assert_eq!(after.template_id, d.template_id);
        assert_eq!(after.sort_order, d.sort_order);

        let body: String =
            sqlx::query_scalar("SELECT body FROM session_documents WHERE id='doc-1'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(body, "# Replaced", "dual-write must reach the index");
    }

    #[tokio::test]
    async fn update_enhanced_doc_errors_when_the_file_is_missing() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1")).await.unwrap();
        let result = store
            .update_enhanced_doc(
                "s1",
                "ghost-doc",
                EnhancedDocPatch {
                    markdown: Some("x".to_string()),
                    ..Default::default()
                },
            )
            .await;
        assert!(result.is_err());
        assert!(!matches!(result.unwrap_err(), StoreError::Conflict(_)));
    }

    #[tokio::test]
    async fn update_enhanced_doc_title_cas_conflict_is_typed_and_changes_nothing() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1")).await.unwrap();
        store.write_enhanced_doc(&doc("s1", "doc-1")).await.unwrap();

        let result = store
            .update_enhanced_doc(
                "s1",
                "doc-1",
                EnhancedDocPatch {
                    title: Some("Hydrated".to_string()),
                    expected_title: Some("Some other title".to_string()),
                    ..Default::default()
                },
            )
            .await;

        assert!(matches!(result.unwrap_err(), StoreError::Conflict(_)));
        let after = store
            .read_enhanced_doc("s1", "doc-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            after.title, "Customer review",
            "a conflict must change nothing"
        );
    }

    #[tokio::test]
    async fn update_enhanced_doc_body_cas_applies_when_current_and_conflicts_when_stale() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1")).await.unwrap();
        store.write_enhanced_doc(&doc("s1", "doc-1")).await.unwrap();

        store
            .update_enhanced_doc(
                "s1",
                "doc-1",
                EnhancedDocPatch {
                    markdown: Some("# Regenerated".to_string()),
                    expected_markdown: Some("# Review\n\n- Point".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let stale = store
            .update_enhanced_doc(
                "s1",
                "doc-1",
                EnhancedDocPatch {
                    markdown: Some("# From a stale run".to_string()),
                    expected_markdown: Some("# Review\n\n- Point".to_string()),
                    ..Default::default()
                },
            )
            .await;
        assert!(matches!(stale.unwrap_err(), StoreError::Conflict(_)));

        let after = store
            .read_enhanced_doc("s1", "doc-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.markdown, "# Regenerated");
    }

    /// The conflict error must be recognizable across the IPC string boundary -- the
    /// frontend distinguishes benign CAS misses (title hydration) from real failures by
    /// this prefix.
    #[test]
    fn conflict_error_stringifies_with_a_stable_prefix() {
        let err = StoreError::Conflict("x".to_string());
        assert!(err.to_string().starts_with("conflict:"));
    }

    #[tokio::test]
    async fn delete_enhanced_doc_moves_file_to_trash_and_hard_deletes_the_row() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1")).await.unwrap();
        store.write_enhanced_doc(&doc("s1", "doc-1")).await.unwrap();

        store.delete_enhanced_doc("s1", "doc-1").await.unwrap();

        assert!(!vault.path().join("sessions/s1/enhanced/doc-1.md").exists());
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert!(
            vault
                .path()
                .join(".trash")
                .join(date)
                .join("sessions/s1/enhanced/doc-1.md")
                .is_file(),
            "deleted doc must be hand-recoverable from trash"
        );
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM session_documents WHERE id='doc-1'")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(n, 0, "no tombstone: the row is hard-deleted");
    }

    #[tokio::test]
    async fn delete_enhanced_doc_on_a_nonexistent_doc_succeeds() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1")).await.unwrap();
        assert!(
            store
                .delete_enhanced_doc("s1", "never-existed")
                .await
                .is_ok()
        );
    }

    #[test]
    fn parse_accepts_a_file_without_frontmatter_as_a_plain_summary() {
        let parsed =
            parse_enhanced_file("d1", "s1", "# Just markdown\n\nDropped in by hand.").unwrap();
        assert_eq!(parsed.kind, "summary");
        assert_eq!(parsed.title, "");
        assert_eq!(parsed.template_id, "");
        assert_eq!(parsed.sort_order, 0);
        assert_eq!(parsed.markdown, "# Just markdown\n\nDropped in by hand.");
    }

    #[test]
    fn parse_degrades_an_unknown_kind_to_summary() {
        let raw = "---\nkind: nonsense\ntitle: T\n---\n\nbody";
        let parsed = parse_enhanced_file("d1", "s1", raw).unwrap();
        assert_eq!(parsed.kind, "summary");
        assert_eq!(parsed.title, "T");
    }

    #[test]
    fn parse_errors_on_malformed_frontmatter() {
        let raw = "---\ntitle: [unclosed\n---\n\nbody";
        assert!(parse_enhanced_file("d1", "s1", raw).is_err());
    }

    #[test]
    fn render_parse_round_trip_preserves_every_field() {
        let d = doc("s1", "doc-1");
        let rendered = render_enhanced_file(&d).unwrap();
        let parsed = parse_enhanced_file("doc-1", "s1", &rendered).unwrap();
        assert_eq!(parsed, d);
    }

    /// A title that needs YAML quoting (colon + quotes) must survive the file round-trip
    /// intact -- the frontmatter is the metadata home, so escaping bugs here would corrupt
    /// real titles.
    #[test]
    fn render_parse_round_trip_survives_a_yaml_hostile_title() {
        let mut d = doc("s1", "doc-1");
        d.title = "Q3: \"planning\" #review --- done".to_string();
        let rendered = render_enhanced_file(&d).unwrap();
        let parsed = parse_enhanced_file("doc-1", "s1", &rendered).unwrap();
        assert_eq!(parsed.title, d.title);
    }
}
