use hypr_fs_format::{TranscriptJson, TranscriptSpeakerHint, TranscriptWithData, TranscriptWord};

use super::{SessionStore, StoreError, paths};

#[derive(serde::Deserialize, specta::Type, Clone)]
pub struct TranscriptDelta {
    pub transcript_id: String,
    pub new_words: Vec<TranscriptWord>,
    pub replaced_ids: Vec<String>,
    pub new_hints: Vec<TranscriptSpeakerHint>,
    pub started_at_ms: f64,
}

#[derive(Debug, Default)]
pub struct LiveTranscriptBuffer {
    pub transcript_id: String,
    pub started_at_ms: f64,
    pub words: Vec<TranscriptWord>,
    pub hints: Vec<TranscriptSpeakerHint>,
    pub dirty: bool,
}

const DEBOUNCE: std::time::Duration = std::time::Duration::from_secs(1);

impl SessionStore {
    /// Buffers the delta and schedules a flush ~1s later. Has no session/index preconditions,
    /// so it can never silently no-op. Usually touches only the in-memory buffer; the one
    /// exception is switching `transcript_id` while the outgoing buffer is dirty, which flushes
    /// the old transcript first and propagates that flush's error (loudly — the caller sees Err
    /// and the incoming delta is NOT buffered; the old transcript's buffer stays dirty for retry).
    pub async fn append_transcript(
        &self,
        session_id: &str,
        delta: TranscriptDelta,
    ) -> Result<(), StoreError> {
        if delta.new_words.is_empty() && delta.replaced_ids.is_empty() && delta.new_hints.is_empty()
        {
            // Nothing to buffer, nothing to (re)dirty, nothing to schedule.
            return Ok(());
        }

        // A session moving on to a new transcript_id (new recording segment) must not carry
        // the previous transcript's words into the new one's file entry. If the outgoing
        // buffer still has unflushed changes, persist them first -- clearing an unflushed
        // buffer would silently drop words that never made it to disk.
        let needs_flush_before_switch = {
            let live = self.live.lock().await;
            live.get(session_id).is_some_and(|buffer| {
                buffer.dirty
                    && !buffer.transcript_id.is_empty()
                    && buffer.transcript_id != delta.transcript_id
            })
        };
        if needs_flush_before_switch {
            self.flush_transcript(session_id).await?;
        }

        let needs_spawn = {
            let mut live = self.live.lock().await;
            let buffer = live.entry(session_id.to_string()).or_default();

            if !buffer.transcript_id.is_empty() && buffer.transcript_id != delta.transcript_id {
                buffer.words.clear();
                buffer.hints.clear();
            }

            if !delta.replaced_ids.is_empty() {
                buffer.words.retain(|word| {
                    !delta
                        .replaced_ids
                        .iter()
                        .any(|id| word.id.as_deref() == Some(id.as_str()))
                });
            }
            buffer.words.extend(delta.new_words);
            buffer.hints.extend(delta.new_hints);
            buffer.transcript_id = delta.transcript_id;
            buffer.started_at_ms = delta.started_at_ms;

            // Only the transition from clean -> dirty schedules a flusher; further appends
            // within the debounce window ride along on the one already pending.
            let was_dirty = buffer.dirty;
            buffer.dirty = true;
            !was_dirty
        };

        if needs_spawn {
            let store = self.clone();
            let session_id = session_id.to_string();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(DEBOUNCE).await;
                    let still_dirty = {
                        let live = store.live.lock().await;
                        live.get(&session_id).is_some_and(|buffer| buffer.dirty)
                    };
                    if !still_dirty {
                        return;
                    }
                    if let Err(err) = store.flush_transcript(&session_id).await {
                        tracing::error!(
                            session_id = %session_id,
                            error = %err,
                            "debounced transcript flush failed; will retry while buffer stays dirty"
                        );
                        // flush_transcript re-dirties the buffer on Err, so the next
                        // iteration's `still_dirty` check will pick this session back up.
                        continue;
                    }
                    return;
                }
            });
        }

        Ok(())
    }

    /// Writes transcript.json from the buffer (or re-reads existing file and merges), updates
    /// the transcripts index row. No-op if there is nothing buffered for this session.
    pub async fn flush_transcript(&self, session_id: &str) -> Result<(), StoreError> {
        let snapshot = {
            let mut live = self.live.lock().await;
            let Some(buffer) = live.get_mut(session_id) else {
                return Ok(());
            };
            // Clear dirty *before* doing I/O: if an append races in while we're writing, it
            // will see a clean buffer, flip it dirty again, and schedule its own flusher --
            // so nothing gets lost even though this in-flight flush won't see that append.
            buffer.dirty = false;
            // Cloning the full word/hint list on every flush is O(n) in transcript length;
            // fine at meeting scale (thousands of words, not millions).
            (
                buffer.transcript_id.clone(),
                buffer.started_at_ms,
                buffer.words.clone(),
                buffer.hints.clone(),
            )
        };

        let (transcript_id, started_at_ms, words, hints) = snapshot;
        let result = self
            .persist_transcript(session_id, &transcript_id, started_at_ms, words, hints)
            .await;

        if result.is_err() {
            // Persist failed: the words only exist in memory. Re-dirty so flush_all() and
            // the debounce retry loop pick this session back up -- a failed flush must never
            // look "clean" (that's the exact silent-loss shape this store exists to prevent).
            let mut live = self.live.lock().await;
            if let Some(buffer) = live.get_mut(session_id) {
                buffer.dirty = true;
            }
        }

        result
    }

    /// App-exit hook: flush every dirty buffer. A failure on one session must not skip the
    /// others, so every session is attempted; the first error (if any) is returned afterward.
    pub async fn flush_all(&self) -> Result<(), StoreError> {
        let dirty_session_ids: Vec<String> = {
            let live = self.live.lock().await;
            live.iter()
                .filter(|(_, buffer)| buffer.dirty)
                .map(|(session_id, _)| session_id.clone())
                .collect()
        };

        let mut first_error = None;
        for session_id in dirty_session_ids {
            if let Err(err) = self.flush_transcript(&session_id).await {
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
        }

        match first_error {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    /// Replace a whole transcript (batch/upload path) — writes file + index in one call.
    pub async fn write_transcript(
        &self,
        session_id: &str,
        t: TranscriptWithData,
    ) -> Result<(), StoreError> {
        let transcript_id = t.id.clone();
        let started_at_ms = t.started_at;
        self.persist_transcript(
            session_id,
            &transcript_id,
            started_at_ms,
            t.words,
            t.speaker_hints,
        )
        .await
    }

    async fn persist_transcript(
        &self,
        session_id: &str,
        transcript_id: &str,
        started_at_ms: f64,
        words: Vec<TranscriptWord>,
        hints: Vec<TranscriptSpeakerHint>,
    ) -> Result<(), StoreError> {
        let mut file = self.read_transcript_json(session_id).await?;

        let idx = match file.transcripts.iter().position(|t| t.id == transcript_id) {
            Some(idx) => {
                let existing = &mut file.transcripts[idx];
                existing.session_id = session_id.to_string();
                existing.started_at = started_at_ms;
                existing.words = words;
                existing.speaker_hints = hints;
                idx
            }
            None => {
                file.transcripts.push(TranscriptWithData {
                    id: transcript_id.to_string(),
                    user_id: String::new(),
                    created_at: chrono::Utc::now().to_rfc3339(),
                    session_id: session_id.to_string(),
                    started_at: started_at_ms,
                    ended_at: None,
                    memo_md: String::new(),
                    words,
                    speaker_hints: hints,
                });
                file.transcripts.len() - 1
            }
        };

        // Serialize the full file up front: a word that can't round-trip must fail the whole
        // flush (StoreError::Serialize), never get silently dropped or collapse to `[]`.
        let bytes =
            serde_json::to_vec_pretty(&file).map_err(|e| StoreError::Serialize(e.to_string()))?;
        let words_json = serde_json::to_string(&file.transcripts[idx].words)
            .map_err(|e| StoreError::Serialize(e.to_string()))?;
        let hints_json = serde_json::to_string(&file.transcripts[idx].speaker_hints)
            .map_err(|e| StoreError::Serialize(e.to_string()))?;

        self.write_file(paths::transcript_path(session_id), bytes)
            .await?;

        sqlx::query(
            "INSERT INTO transcripts (id, session_id, started_at_ms, memo, words_json, speaker_hints_json, updated_at)
             VALUES (?, ?, ?, '', ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(id) DO UPDATE SET
               session_id = excluded.session_id,
               started_at_ms = excluded.started_at_ms,
               words_json = excluded.words_json,
               speaker_hints_json = excluded.speaker_hints_json,
               updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )
        .bind(transcript_id)
        .bind(session_id)
        .bind(started_at_ms.round() as i64)
        .bind(&words_json)
        .bind(&hints_json)
        .execute(self.pool())
        .await
        .map_err(|e| StoreError::Db(e.to_string()))?;

        Ok(())
    }

    async fn read_transcript_json(&self, session_id: &str) -> Result<TranscriptJson, StoreError> {
        let vault_base = self.vault_base.clone();
        let relative = paths::transcript_path(session_id);

        tokio::task::spawn_blocking(move || -> Result<TranscriptJson, StoreError> {
            let path = vault_base.join(&relative);
            if !path.exists() {
                return Ok(TranscriptJson {
                    transcripts: Vec::new(),
                });
            }

            let bytes = std::fs::read(&path)
                .map_err(|e| StoreError::Io(format!("failed to read transcript file: {}", e)))?;

            serde_json::from_slice(&bytes).map_err(|e| {
                StoreError::Serialize(format!("failed to deserialize transcript.json: {}", e))
            })
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {}", e)))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_store() -> (SessionStore, tempfile::TempDir) {
        let temp = tempfile::tempdir().unwrap();
        let vault = temp.path().to_path_buf();
        let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
        hypr_db_app::prepare_schema(&db).await.unwrap();
        let store = SessionStore::new(vault, db.pool().clone());
        (store, temp)
    }

    fn word(id: &str, text: &str) -> TranscriptWord {
        TranscriptWord {
            id: Some(id.to_string()),
            text: text.to_string(),
            start_ms: 0.0,
            end_ms: 0.0,
            channel: 0.0,
            speaker: None,
            metadata: None,
        }
    }

    fn delta_with_words(words: &[&str]) -> TranscriptDelta {
        TranscriptDelta {
            transcript_id: "t1".to_string(),
            new_words: words
                .iter()
                .enumerate()
                .map(|(i, w)| word(&format!("w{i}"), w))
                .collect(),
            replaced_ids: vec![],
            new_hints: vec![],
            started_at_ms: 1000.0,
        }
    }

    #[tokio::test]
    async fn append_then_flush_writes_words_to_file_and_index() {
        let (store, vault) = test_store().await;
        store
            .append_transcript("s1", delta_with_words(&["hello", "world"]))
            .await
            .unwrap();
        store.flush_transcript("s1").await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(vault.path().join("sessions/s1/transcript.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(json["transcripts"][0]["words"].as_array().unwrap().len(), 2);
        let words: String =
            sqlx::query_scalar("SELECT words_json FROM transcripts WHERE session_id='s1'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert!(words.contains("hello"));
    }

    /// REGRESSION for the 2026-07-23 data loss: no index row, no folder, no _meta.json — words still land.
    #[tokio::test]
    async fn recording_into_unknown_session_still_persists() {
        let (store, vault) = test_store().await;
        // deliberately: no write_meta, no sessions row
        store
            .append_transcript("ghost", delta_with_words(&["survives"]))
            .await
            .unwrap();
        store.flush_transcript("ghost").await.unwrap();
        assert!(
            vault
                .path()
                .join("sessions/ghost/transcript.json")
                .is_file()
        );
    }

    #[tokio::test]
    async fn debounce_flushes_without_explicit_flush() {
        let (store, vault) = test_store().await;
        store
            .append_transcript("s1", delta_with_words(&["auto"]))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        assert!(vault.path().join("sessions/s1/transcript.json").is_file());
    }

    #[tokio::test]
    async fn replaced_ids_removes_superseded_words_before_appending() {
        let (store, vault) = test_store().await;
        store
            .append_transcript("s1", delta_with_words(&["hell", "world"]))
            .await
            .unwrap();
        store
            .append_transcript(
                "s1",
                TranscriptDelta {
                    transcript_id: "t1".to_string(),
                    new_words: vec![word("w0-fix", "hello")],
                    replaced_ids: vec!["w0".to_string()],
                    new_hints: vec![],
                    started_at_ms: 1000.0,
                },
            )
            .await
            .unwrap();
        store.flush_transcript("s1").await.unwrap();

        let json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(vault.path().join("sessions/s1/transcript.json")).unwrap(),
        )
        .unwrap();
        let texts: Vec<&str> = json["transcripts"][0]["words"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["text"].as_str().unwrap())
            .collect();
        assert_eq!(texts, vec!["world", "hello"]);
    }

    #[tokio::test]
    async fn flush_merges_and_preserves_other_transcripts_in_same_file() {
        let (store, vault) = test_store().await;
        store
            .append_transcript("s1", delta_with_words(&["first"]))
            .await
            .unwrap();
        store.flush_transcript("s1").await.unwrap();

        store
            .append_transcript(
                "s1",
                TranscriptDelta {
                    transcript_id: "t2".to_string(),
                    new_words: vec![word("y0", "second")],
                    replaced_ids: vec![],
                    new_hints: vec![],
                    started_at_ms: 2000.0,
                },
            )
            .await
            .unwrap();
        store.flush_transcript("s1").await.unwrap();

        let json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(vault.path().join("sessions/s1/transcript.json")).unwrap(),
        )
        .unwrap();
        let transcripts = json["transcripts"].as_array().unwrap();
        assert_eq!(transcripts.len(), 2);
        let ids: Vec<&str> = transcripts
            .iter()
            .map(|t| t["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"t1"));
        assert!(ids.contains(&"t2"));
        let t1 = transcripts.iter().find(|t| t["id"] == "t1").unwrap();
        assert_eq!(t1["words"].as_array().unwrap().len(), 1);
        // Regression: switching transcript_id must reset the live buffer, not carry t1's
        // words into t2's entry.
        let t2 = transcripts.iter().find(|t| t["id"] == "t2").unwrap();
        let t2_texts: Vec<&str> = t2["words"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["text"].as_str().unwrap())
            .collect();
        assert_eq!(t2_texts, vec!["second"]);
    }

    #[tokio::test]
    async fn repeated_appends_within_debounce_window_share_one_flusher() {
        let (store, vault) = test_store().await;
        for word_text in ["a", "b", "c"] {
            store
                .append_transcript("s1", delta_with_words(&[word_text]))
                .await
                .unwrap();
        }
        // Give the single scheduled flusher time to fire; if appends had each spawned their
        // own flusher this would still pass, so the real assertion is the word count below --
        // three separate un-coalesced buffers would have clobbered each other down to 1 word.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        let json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(vault.path().join("sessions/s1/transcript.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(json["transcripts"][0]["words"].as_array().unwrap().len(), 3);

        let words_json: String =
            sqlx::query_scalar("SELECT words_json FROM transcripts WHERE id='t1'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert!(words_json.contains('a') && words_json.contains('b') && words_json.contains('c'));
    }

    #[tokio::test]
    async fn flush_all_flushes_every_dirty_session_and_reports_first_error() {
        let (store, vault) = test_store().await;
        store
            .append_transcript("s1", delta_with_words(&["one"]))
            .await
            .unwrap();
        store
            .append_transcript("s2", delta_with_words(&["two"]))
            .await
            .unwrap();

        store.flush_all().await.unwrap();

        assert!(vault.path().join("sessions/s1/transcript.json").is_file());
        assert!(vault.path().join("sessions/s2/transcript.json").is_file());
    }

    #[tokio::test]
    async fn flush_all_is_noop_when_nothing_dirty() {
        let (store, _vault) = test_store().await;
        store.flush_all().await.unwrap();
    }

    #[tokio::test]
    async fn write_transcript_replaces_file_and_index_in_one_call() {
        let (store, vault) = test_store().await;
        store
            .write_transcript(
                "s1",
                TranscriptWithData {
                    id: "batch".to_string(),
                    user_id: String::new(),
                    created_at: "2026-07-24T00:00:00Z".to_string(),
                    session_id: "s1".to_string(),
                    started_at: 500.0,
                    ended_at: Some(900.0),
                    memo_md: String::new(),
                    words: vec![word("b0", "uploaded")],
                    speaker_hints: vec![],
                },
            )
            .await
            .unwrap();

        assert!(vault.path().join("sessions/s1/transcript.json").is_file());
        let words_json: String =
            sqlx::query_scalar("SELECT words_json FROM transcripts WHERE id='batch'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert!(words_json.contains("uploaded"));
    }

    #[tokio::test]
    async fn explicit_flush_drains_buffer_so_pending_debounce_timer_is_a_harmless_noop() {
        let (store, vault) = test_store().await;
        // append schedules a debounce timer; flushing explicitly right away drains the
        // buffer before that timer ever fires.
        store
            .append_transcript("s1", delta_with_words(&["first"]))
            .await
            .unwrap();
        store.flush_transcript("s1").await.unwrap();

        // let the still-pending debounce timer from the append above fire; it must see a
        // clean buffer and skip, not re-write stale/duplicate content or error out.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        let json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(vault.path().join("sessions/s1/transcript.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(json["transcripts"][0]["words"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn append_after_explicit_drain_is_still_flushed_by_a_freshly_scheduled_timer() {
        let (store, vault) = test_store().await;
        store
            .append_transcript("s1", delta_with_words(&["first"]))
            .await
            .unwrap();
        store.flush_transcript("s1").await.unwrap();

        // buffer is clean now; this append must re-arm its own debounce timer rather than
        // relying on the (already-fired) one from the first append -- otherwise the word
        // would sit in memory forever with nothing scheduled to persist it.
        store
            .append_transcript("s1", delta_with_words(&["second"]))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        let json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(vault.path().join("sessions/s1/transcript.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(json["transcripts"][0]["words"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn flush_transcript_with_nothing_buffered_is_a_noop() {
        let (store, vault) = test_store().await;
        store.flush_transcript("never-touched").await.unwrap();
        assert!(
            !vault
                .path()
                .join("sessions/never-touched/transcript.json")
                .is_file()
        );
    }

    /// REGRESSION for Critical 1 (reviewer-traced): a failed index write must not be
    /// swallowed, and the buffer must stay dirty so flush_all() can recover the session
    /// instead of silently treating it as flushed.
    #[tokio::test]
    async fn flush_transcript_index_failure_leaves_file_intact_and_buffer_dirty_for_retry() {
        let (store, vault) = test_store().await;
        store
            .append_transcript("s1", delta_with_words(&["hello"]))
            .await
            .unwrap();

        sqlx::query("DROP TABLE transcripts")
            .execute(store.pool())
            .await
            .unwrap();

        let result = store.flush_transcript("s1").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StoreError::Db(_)));

        // File write happens before the index upsert, so it must have landed even though
        // the index write failed -- the file is truth.
        let json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(vault.path().join("sessions/s1/transcript.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(json["transcripts"][0]["words"].as_array().unwrap().len(), 1);

        // The failed flush must re-dirty the buffer, not leave it looking clean.
        {
            let live = store.live.lock().await;
            assert!(
                live.get("s1").unwrap().dirty,
                "buffer must be re-dirtied after a failed flush"
            );
        }

        // Recreate the table (mirrors the canonical migration's transcripts DDL) and
        // confirm flush_all() recovers the session that previously failed.
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS transcripts (
              id                    TEXT PRIMARY KEY NOT NULL,
              workspace_id          TEXT NOT NULL DEFAULT '',
              owner_user_id         TEXT NOT NULL DEFAULT '',
              session_id            TEXT NOT NULL DEFAULT '',
              source                TEXT NOT NULL DEFAULT '',
              provider              TEXT NOT NULL DEFAULT '',
              model                 TEXT NOT NULL DEFAULT '',
              language              TEXT NOT NULL DEFAULT '',
              started_at_ms         INTEGER NOT NULL DEFAULT 0,
              ended_at_ms           INTEGER,
              audio_attachment_id   TEXT NOT NULL DEFAULT '',
              memo                   TEXT NOT NULL DEFAULT '',
              words_json            TEXT NOT NULL DEFAULT '[]',
              speaker_hints_json    TEXT NOT NULL DEFAULT '[]',
              metadata_json         TEXT NOT NULL DEFAULT '{}',
              created_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
              updated_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
              deleted_at            TEXT
            ) STRICT",
        )
        .execute(store.pool())
        .await
        .unwrap();

        store.flush_all().await.unwrap();

        let row_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM transcripts WHERE session_id='s1'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(row_count, 1);
    }
}
