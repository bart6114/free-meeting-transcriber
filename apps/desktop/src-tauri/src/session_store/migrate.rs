//! Task 12: one-time final export sweep.
//!
//! The session store (Tasks 5-11) owns every *new* write, and startup runs
//! `rebuild::rebuild_index` to index whatever's already on disk. That leaves
//! exactly one gap before the old DB-to-vault machinery (`vault_export.rs`,
//! `crates/fs-sync-core`) is deleted in Task 13: DB content that predates the
//! store and was never exported to files at all — most notably transcripts,
//! since `export::render_transcripts`'s `unwrap_or_default()` silently drops
//! a `transcripts.words_json` row that doesn't strict-parse as
//! `Vec<TranscriptWord>` (see `repair_words_json`'s doc for exactly which
//! shapes fail, and why "integers" alone turned out not to be one of them).
//!
//! `run_once` drains the *old* export machinery one final time — reusing
//! `vault_export::enqueue_all_entities` + `vault_export::drain_queue`
//! directly rather than spawning `vault_export::run`'s long-lived worker
//! task — after first repairing any `words_json` row it can. It runs exactly
//! once per vault, gated by a marker file at the vault root, and never runs
//! again once that marker exists.

use std::path::Path;

use sqlx::SqlitePool;
use tauri::AppHandle;

use crate::vault_export;

/// Plain top-level marker file, not session content — written directly with
/// `std::fs::write` (via `spawn_blocking`), the same way
/// `vault_export.rs::ensure_first_run_full_export` writes its own
/// `.fmt-export-version` marker. Not routed through the session store: the
/// store's write paths are for `sessions/**` content, and this sweep runs
/// *before* the store's `rebuild_index`, so nothing has indexed anything
/// yet.
const MARKER_FILENAME: &str = ".store-migrated-v1";
const MARKER_CONTENT: &str = "1";

/// Defends against a permanently-stuck entity looping forever — `drain_with`
/// (see `vault_export.rs`) already makes maximal progress within a single
/// call (it keeps re-batching until nothing further can be acked), so this
/// mostly bounds pointless extra passes once every failure is backed off;
/// see `run_once`'s loop for the exact behavior.
const MAX_DRAIN_PASSES: usize = 5;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct MigrateReport {
    /// `true` when `run_once` returned immediately because the marker file
    /// already existed — every other field is left at its default in that
    /// case.
    pub skipped_marker_present: bool,
    /// Count of `transcripts.words_json` rows rewritten in place after
    /// failing strict `Vec<TranscriptWord>` parsing but succeeding a lenient,
    /// field-coercing reparse.
    pub repaired_words_json: usize,
    /// `transcripts.id` for rows that failed even the lenient reparse. Left
    /// untouched in the DB; their owning session's export is skipped for
    /// this sweep (see `run_once`) so the old exporter's `unwrap_or_default`
    /// can't silently write an empty word list. Recorded here as the
    /// inventory Task 14's hardening pass needs.
    pub unparseable_words_json: Vec<String>,
    /// How many `drain_queue` calls this sweep actually made (capped at
    /// `MAX_DRAIN_PASSES`).
    pub drain_passes: usize,
    /// Human-readable notes for anything that didn't fully succeed: entities
    /// still stuck in `vault_export_dirty` after `MAX_DRAIN_PASSES`, and
    /// sessions skipped because one of their transcripts stayed unparseable.
    /// One bad row is logged here and the sweep continues — it never aborts
    /// the whole pass.
    pub export_errors: Vec<String>,
}

/// Runs the sweep exactly once per vault. Returns early (a no-op,
/// `MigrateReport::skipped_marker_present`) if `<vault_base>/.store-migrated-v1`
/// already exists. Order matters: repair `words_json` *before* enqueuing/
/// draining, so the drain never has a chance to render a still-broken row as
/// an empty word list.
pub async fn run_once<R: tauri::Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    vault_base: &Path,
) -> Result<MigrateReport, String> {
    let marker = vault_base.join(MARKER_FILENAME);
    let marker_for_check = marker.clone();
    let already_migrated = tokio::task::spawn_blocking(move || marker_for_check.exists())
        .await
        .map_err(|error| error.to_string())?;

    if already_migrated {
        return Ok(MigrateReport {
            skipped_marker_present: true,
            ..Default::default()
        });
    }

    let mut report = MigrateReport::default();

    let sessions_to_skip = repair_transcripts_words_json(pool, &mut report).await?;

    vault_export::enqueue_all_entities(pool)
        .await
        .map_err(|error| error.to_string())?;

    if !sessions_to_skip.is_empty() {
        let mut tx = pool.begin().await.map_err(|error| error.to_string())?;
        for session_id in &sessions_to_skip {
            sqlx::query(
                "DELETE FROM vault_export_dirty WHERE entity_type = 'session' AND entity_id = ?",
            )
            .bind(session_id)
            .execute(&mut *tx)
            .await
            .map_err(|error| error.to_string())?;
        }
        tx.commit().await.map_err(|error| error.to_string())?;
    }

    let mut backoff = vault_export::RetryBackoff::new();
    for _ in 0..MAX_DRAIN_PASSES {
        report.drain_passes += 1;
        vault_export::drain_queue(app, pool, &mut backoff)
            .await
            .map_err(|error| error.to_string())?;

        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vault_export_dirty")
            .fetch_one(pool)
            .await
            .map_err(|error| error.to_string())?;
        if remaining == 0 {
            break;
        }
    }

    let stuck: Vec<(String, String)> = sqlx::query_as(
        "SELECT entity_type, entity_id FROM vault_export_dirty ORDER BY entity_type, entity_id",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| error.to_string())?;
    for (entity_type, entity_id) in stuck {
        let message = format!(
            "{entity_type} {entity_id} still queued after {} drain pass(es); left for the live export worker to retry",
            report.drain_passes
        );
        tracing::warn!("{message}");
        report.export_errors.push(message);
    }

    write_marker(&marker).await?;

    Ok(report)
}

async fn write_marker(marker: &Path) -> Result<(), String> {
    let vault_base = marker
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| marker.to_path_buf());
    let marker = marker.to_path_buf();

    tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&vault_base)
            .and_then(|()| std::fs::write(&marker, MARKER_CONTENT))
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Repairs every live `transcripts.words_json` row that fails strict
/// `Vec<TranscriptWord>` parsing but succeeds a lenient reparse, updating the
/// DB row in place (one transaction per row). Returns the distinct
/// `session_id`s of any row that stayed unparseable even after that lenient
/// pass — `run_once` excludes those sessions from this sweep's drain
/// entirely, so the old exporter never gets a chance to render their
/// `transcript.json` with the broken word list silently blanked out
/// (`export::render_transcripts` uses `unwrap_or_default()`, which this
/// module intentionally does not touch — see the module doc).
async fn repair_transcripts_words_json(
    pool: &SqlitePool,
    report: &mut MigrateReport,
) -> Result<Vec<String>, String> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, session_id, words_json FROM transcripts WHERE deleted_at IS NULL",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| error.to_string())?;

    let mut sessions_to_skip = Vec::new();

    for (transcript_id, session_id, words_json) in rows {
        match repair_words_json(&words_json) {
            RepairOutcome::AlreadyValid => {}
            RepairOutcome::Repaired(new_words_json) => {
                let mut tx = pool.begin().await.map_err(|error| error.to_string())?;
                sqlx::query("UPDATE transcripts SET words_json = ? WHERE id = ?")
                    .bind(&new_words_json)
                    .bind(&transcript_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| error.to_string())?;
                tx.commit().await.map_err(|error| error.to_string())?;
                report.repaired_words_json += 1;
            }
            RepairOutcome::Unrepairable(reason) => {
                tracing::warn!(
                    transcript_id = %transcript_id,
                    session_id = %session_id,
                    reason = %reason,
                    "words_json failed even lenient repair; leaving the DB row untouched and excluding its session from this sweep"
                );
                report.unparseable_words_json.push(transcript_id.clone());
                report.export_errors.push(format!(
                    "session {session_id} export skipped this sweep: transcript {transcript_id} words_json unparseable even after lenient repair ({reason})"
                ));
                sessions_to_skip.push(session_id);
            }
        }
    }

    sessions_to_skip.sort();
    sessions_to_skip.dedup();
    Ok(sessions_to_skip)
}

enum RepairOutcome {
    AlreadyValid,
    Repaired(String),
    Unrepairable(String),
}

/// Pure, unit-testable core of the repair pass.
///
/// A plain JSON integer (`"start_ms":0`) already strict-parses fine into an
/// `f64` field — `serde_json` accepts any JSON number for a Rust `f64`
/// target regardless of whether it's written with a decimal point (verified
/// directly against this crate's `serde_json` version before writing this).
/// What actually fails strict `Vec<TranscriptWord>` parsing, and is worth
/// recovering here, is a `start_ms`/`end_ms`/`channel` that's missing,
/// `null`, or — the shape this function is named for — serialized as a JSON
/// *string* instead of a number (a plausible legacy round-trip bug: some
/// pre-store write path stringified the millisecond value instead of
/// emitting a real JSON number). All three are coerced to a proper `f64`
/// JSON number; every other field on the word object (`id`, `text`,
/// `speaker`, `metadata`, or anything not known to `TranscriptWord` at all)
/// is left byte-for-byte as it was, so nothing forward-compatible is lost by
/// round-tripping through a typed struct.
fn repair_words_json(words_json: &str) -> RepairOutcome {
    if serde_json::from_str::<Vec<hypr_fs_format::TranscriptWord>>(words_json).is_ok() {
        return RepairOutcome::AlreadyValid;
    }

    let mut values: Vec<serde_json::Value> = match serde_json::from_str(words_json) {
        Ok(values) => values,
        Err(error) => {
            return RepairOutcome::Unrepairable(format!("not a JSON array of words: {error}"));
        }
    };

    for (index, value) in values.iter_mut().enumerate() {
        let Some(object) = value.as_object_mut() else {
            return RepairOutcome::Unrepairable(format!("word #{index} is not a JSON object"));
        };

        for field in ["start_ms", "end_ms", "channel"] {
            let Some(coerced) = coerce_numeric_field(object.get(field)) else {
                return RepairOutcome::Unrepairable(format!(
                    "word #{index}'s `{field}` is not a coercible number"
                ));
            };
            object.insert(field.to_string(), coerced);
        }
    }

    let repaired = match serde_json::to_string(&values) {
        Ok(repaired) => repaired,
        Err(error) => {
            return RepairOutcome::Unrepairable(format!("failed to re-serialize: {error}"));
        }
    };

    match serde_json::from_str::<Vec<hypr_fs_format::TranscriptWord>>(&repaired) {
        Ok(_) => RepairOutcome::Repaired(repaired),
        Err(error) => RepairOutcome::Unrepairable(format!(
            "still fails strict parsing after coercion: {error}"
        )),
    }
}

/// Coerces one word field to a JSON number: a JSON number passes through
/// as-is (still handles a legitimate integer, harmlessly); a JSON string
/// containing a number is parsed; a missing key or explicit `null` defaults
/// to `0.0` (mirroring `TranscriptWord`'s other already-defaulted optional
/// fields); anything else (bool, array, object) can't be coerced.
fn coerce_numeric_field(value: Option<&serde_json::Value>) -> Option<serde_json::Value> {
    match value {
        Some(serde_json::Value::Number(number)) => {
            let float = number.as_f64()?;
            serde_json::Number::from_f64(float).map(serde_json::Value::Number)
        }
        Some(serde_json::Value::String(text)) => {
            let float: f64 = text.trim().parse().ok()?;
            serde_json::Number::from_f64(float).map(serde_json::Value::Number)
        }
        None | Some(serde_json::Value::Null) => Some(serde_json::Value::Number(0.into())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use tauri::Manager;

    use super::*;

    async fn test_db() -> hypr_db_core::Db {
        let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
        hypr_db_app::prepare_schema(&db).await.unwrap();
        db
    }

    /// Mirrors `plugins/tantivy/tests/index_location.rs`'s workaround:
    /// `tauri_plugin_notify::init()`/`tauri_plugin_settings::init()` are
    /// pinned to (or default-instantiated against) `tauri::Wry`, which can't
    /// be attached via `.plugin()` to a `tauri::test::mock_builder()` app
    /// (`MockRuntime`). Managing the states those plugins' own `setup()`
    /// hooks would have managed sidesteps that entirely, and needs no env
    /// var tricks (parallel-test-safe, unlike overriding `FMTR_VAULT_BASE`).
    fn mock_app(vault_base: &std::path::Path) -> tauri::App<tauri::test::MockRuntime> {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        app.manage(tauri_plugin_settings::StartupSnapshot::new(
            vault_base.to_path_buf(),
        ));
        app.manage(tauri_plugin_notify::WatcherState::empty());
        app
    }

    async fn seed_session_and_transcript(pool: &SqlitePool, session_id: &str, words_json: &str) {
        sqlx::query("INSERT INTO sessions (id, title, created_at) VALUES (?, 'Old session', '2026-01-01T00:00:00.000Z')")
            .bind(session_id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO transcripts (id, session_id, started_at_ms, words_json)
             VALUES (?, ?, 0, ?)",
        )
        .bind(format!("{session_id}-transcript"))
        .bind(session_id)
        .bind(words_json)
        .execute(pool)
        .await
        .unwrap();
    }

    fn file_mtime(path: &std::path::Path) -> std::time::SystemTime {
        std::fs::metadata(path).unwrap().modified().unwrap()
    }

    // ------------------------------------------------------------------
    // repair_words_json: pure unit tests
    // ------------------------------------------------------------------

    #[test]
    fn plain_json_integers_already_strict_parse_and_need_no_repair() {
        let json = r#"[{"text":"hi","start_ms":0,"end_ms":100,"channel":0}]"#;
        assert!(matches!(
            repair_words_json(json),
            RepairOutcome::AlreadyValid
        ));
    }

    #[test]
    fn numeric_strings_are_coerced_to_json_numbers() {
        let json = r#"[{"id":"w1","text":"hello","start_ms":"0","end_ms":"500","channel":"0"}]"#;
        let RepairOutcome::Repaired(repaired) = repair_words_json(json) else {
            panic!("expected a repair");
        };
        let words: Vec<hypr_fs_format::TranscriptWord> = serde_json::from_str(&repaired).unwrap();
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].start_ms, 0.0);
        assert_eq!(words[0].end_ms, 500.0);
        assert_eq!(words[0].channel, 0.0);
        assert_eq!(words[0].id.as_deref(), Some("w1"));
    }

    #[test]
    fn missing_and_null_numeric_fields_default_to_zero() {
        let json = r#"[{"text":"hi","start_ms":null,"end_ms":100}]"#;
        let RepairOutcome::Repaired(repaired) = repair_words_json(json) else {
            panic!("expected a repair");
        };
        let words: Vec<hypr_fs_format::TranscriptWord> = serde_json::from_str(&repaired).unwrap();
        assert_eq!(words[0].start_ms, 0.0);
        assert_eq!(words[0].end_ms, 100.0);
        assert_eq!(words[0].channel, 0.0);
    }

    #[test]
    fn garbage_json_is_unrepairable() {
        assert!(matches!(
            repair_words_json("not json at all"),
            RepairOutcome::Unrepairable(_)
        ));
    }

    #[test]
    fn a_non_array_top_level_value_is_unrepairable() {
        assert!(matches!(
            repair_words_json(r#"{"oops":true}"#),
            RepairOutcome::Unrepairable(_)
        ));
    }

    #[test]
    fn a_non_coercible_field_type_is_unrepairable() {
        let json = r#"[{"text":"hi","start_ms":[1,2],"end_ms":100,"channel":0}]"#;
        assert!(matches!(
            repair_words_json(json),
            RepairOutcome::Unrepairable(_)
        ));
    }

    #[test]
    fn a_missing_required_text_field_is_unrepairable_even_after_numeric_coercion() {
        let json = r#"[{"start_ms":0,"end_ms":100,"channel":0}]"#;
        assert!(matches!(
            repair_words_json(json),
            RepairOutcome::Unrepairable(_)
        ));
    }

    // ------------------------------------------------------------------
    // run_once: integration tests
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn first_run_repairs_legacy_words_json_exports_transcript_and_writes_marker() {
        let db = test_db().await;
        let vault = tempfile::tempdir().unwrap();
        seed_session_and_transcript(
            db.pool(),
            "session-legacy-1",
            r#"[{"id":"w1","text":"hello","start_ms":"0","end_ms":"500","channel":"0"}]"#,
        )
        .await;

        let app = mock_app(vault.path());
        let handle = app.handle().clone();

        let report = super::run_once(&handle, db.pool(), vault.path())
            .await
            .unwrap();

        assert!(!report.skipped_marker_present);
        assert_eq!(report.repaired_words_json, 1);
        assert!(report.unparseable_words_json.is_empty());
        assert!(report.export_errors.is_empty());

        let transcript_path = vault
            .path()
            .join("sessions/session-legacy-1/transcript.json");
        assert!(
            transcript_path.is_file(),
            "transcript.json must be exported"
        );
        let content = std::fs::read_to_string(&transcript_path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&content).unwrap();
        let words = value["transcripts"][0]["words"].as_array().unwrap();
        assert_eq!(words.len(), 1);
        assert_eq!(words[0]["text"], "hello");
        assert_eq!(words[0]["start_ms"], 0.0);
        assert_eq!(words[0]["end_ms"], 500.0);

        let marker = vault.path().join(MARKER_FILENAME);
        assert!(marker.is_file(), "marker file must be written");

        let repaired_words_json: String = sqlx::query_scalar(
            "SELECT words_json FROM transcripts WHERE id = 'session-legacy-1-transcript'",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert!(
            serde_json::from_str::<Vec<hypr_fs_format::TranscriptWord>>(&repaired_words_json)
                .is_ok(),
            "the DB row itself must now strict-parse"
        );
    }

    #[tokio::test]
    async fn second_run_is_a_no_op_and_touches_no_files() {
        let db = test_db().await;
        let vault = tempfile::tempdir().unwrap();
        seed_session_and_transcript(
            db.pool(),
            "session-legacy-2",
            r#"[{"text":"hi","start_ms":"0","end_ms":"100","channel":"0"}]"#,
        )
        .await;

        let app = mock_app(vault.path());
        let handle = app.handle().clone();

        super::run_once(&handle, db.pool(), vault.path())
            .await
            .unwrap();

        let meta_path = vault.path().join("sessions/session-legacy-2/_meta.json");
        let transcript_path = vault
            .path()
            .join("sessions/session-legacy-2/transcript.json");
        let meta_mtime_before = file_mtime(&meta_path);
        let transcript_mtime_before = file_mtime(&transcript_path);

        let report = super::run_once(&handle, db.pool(), vault.path())
            .await
            .unwrap();

        assert!(report.skipped_marker_present);
        assert_eq!(report.repaired_words_json, 0);
        assert_eq!(report.drain_passes, 0);

        assert_eq!(file_mtime(&meta_path), meta_mtime_before);
        assert_eq!(file_mtime(&transcript_path), transcript_mtime_before);
    }

    #[tokio::test]
    async fn marker_already_present_short_circuits_before_touching_anything() {
        let db = test_db().await;
        let vault = tempfile::tempdir().unwrap();
        seed_session_and_transcript(
            db.pool(),
            "session-legacy-3",
            r#"[{"text":"hi","start_ms":"0","end_ms":"100","channel":"0"}]"#,
        )
        .await;
        std::fs::write(vault.path().join(MARKER_FILENAME), MARKER_CONTENT).unwrap();

        let app = mock_app(vault.path());
        let handle = app.handle().clone();

        let report = super::run_once(&handle, db.pool(), vault.path())
            .await
            .unwrap();

        assert_eq!(
            report,
            MigrateReport {
                skipped_marker_present: true,
                ..Default::default()
            }
        );
        assert!(
            !vault
                .path()
                .join("sessions/session-legacy-3/transcript.json")
                .exists(),
            "nothing should have been exported"
        );

        let words_json: String = sqlx::query_scalar(
            "SELECT words_json FROM transcripts WHERE id = 'session-legacy-3-transcript'",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(
            words_json, r#"[{"text":"hi","start_ms":"0","end_ms":"100","channel":"0"}]"#,
            "the row must be untouched"
        );
    }

    #[tokio::test]
    async fn a_row_unparseable_even_leniently_is_left_untouched_and_never_exported_empty() {
        let db = test_db().await;
        let vault = tempfile::tempdir().unwrap();
        seed_session_and_transcript(db.pool(), "session-broken", "not valid json words at all")
            .await;

        let app = mock_app(vault.path());
        let handle = app.handle().clone();

        let report = super::run_once(&handle, db.pool(), vault.path())
            .await
            .unwrap();

        assert!(!report.skipped_marker_present);
        assert_eq!(report.repaired_words_json, 0);
        assert_eq!(
            report.unparseable_words_json,
            vec!["session-broken-transcript".to_string()]
        );
        assert!(
            !report.export_errors.is_empty(),
            "the skipped session must be logged"
        );

        assert!(
            !vault
                .path()
                .join("sessions/session-broken/transcript.json")
                .exists(),
            "an unparseable row must never be exported as an empty word list"
        );
        assert!(
            !vault
                .path()
                .join("sessions/session-broken/_meta.json")
                .exists(),
            "the whole session is skipped this sweep, not just its transcript"
        );

        let words_json: String = sqlx::query_scalar(
            "SELECT words_json FROM transcripts WHERE id = 'session-broken-transcript'",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(words_json, "not valid json words at all");

        let marker = vault.path().join(MARKER_FILENAME);
        assert!(
            marker.is_file(),
            "one bad row must not block the rest of the sweep from completing"
        );
    }

    #[tokio::test]
    async fn one_bad_session_does_not_block_a_good_sessions_export() {
        let db = test_db().await;
        let vault = tempfile::tempdir().unwrap();
        seed_session_and_transcript(db.pool(), "session-broken", "garbage").await;
        seed_session_and_transcript(
            db.pool(),
            "session-good",
            r#"[{"text":"fine","start_ms":0,"end_ms":10,"channel":0}]"#,
        )
        .await;

        let app = mock_app(vault.path());
        let handle = app.handle().clone();

        let report = super::run_once(&handle, db.pool(), vault.path())
            .await
            .unwrap();

        assert_eq!(report.unparseable_words_json.len(), 1);
        assert!(
            vault
                .path()
                .join("sessions/session-good/transcript.json")
                .is_file()
        );
        assert!(
            !vault
                .path()
                .join("sessions/session-broken/transcript.json")
                .exists()
        );
    }
}
