//! One-time `transcripts.words_json` repair, gated by a marker file.
//!
//! Some pre-store write paths left `words_json` rows that fail strict
//! `Vec<TranscriptWord>` parsing (missing/null/string-typed numeric fields —
//! see `repair_words_json`'s doc). `run_once` rewrites every row a lenient
//! reparse can recover, exactly once per vault: a marker file at the vault
//! root records that the sweep already ran, and its presence short-circuits
//! every later startup.

use std::path::Path;

use sqlx::SqlitePool;

/// Plain top-level marker file, not session content — written directly with
/// `std::fs::write` (via `spawn_blocking`). Not routed through the session
/// store: the store's write paths are for `sessions/**` content, and this
/// sweep runs *before* the store's `rebuild_index`, so nothing has indexed
/// anything yet.
const MARKER_FILENAME: &str = ".store-migrated-v1";
const MARKER_CONTENT: &str = "1";

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
    /// untouched in the DB; recorded here as the inventory Task 14's
    /// hardening pass needs. A bad row never aborts the sweep or blocks the
    /// marker.
    pub unparseable_words_json: Vec<String>,
}

/// Runs the repair sweep exactly once per vault. Returns early (a no-op,
/// `MigrateReport::skipped_marker_present`) if `<vault_base>/.store-migrated-v1`
/// already exists.
pub async fn run_once(pool: &SqlitePool, vault_base: &Path) -> Result<MigrateReport, String> {
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
    repair_transcripts_words_json(pool, &mut report).await?;
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
/// DB row in place. Rows that stay unparseable even after the lenient pass
/// are left untouched and recorded in the report.
async fn repair_transcripts_words_json(
    pool: &SqlitePool,
    report: &mut MigrateReport,
) -> Result<(), String> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, session_id, words_json FROM transcripts WHERE deleted_at IS NULL",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| error.to_string())?;

    for (transcript_id, session_id, words_json) in rows {
        match repair_words_json(&words_json) {
            RepairOutcome::AlreadyValid => {}
            RepairOutcome::Repaired(new_words_json) => {
                // No read-then-update race to guard against: this sweep runs
                // during startup, before the store or any UI writer exists,
                // so nothing else can touch the row between the SELECT above
                // and this UPDATE.
                sqlx::query("UPDATE transcripts SET words_json = ? WHERE id = ?")
                    .bind(&new_words_json)
                    .bind(&transcript_id)
                    .execute(pool)
                    .await
                    .map_err(|error| error.to_string())?;
                report.repaired_words_json += 1;
            }
            RepairOutcome::Unrepairable(reason) => {
                tracing::warn!(
                    transcript_id = %transcript_id,
                    session_id = %session_id,
                    reason = %reason,
                    "words_json failed even lenient repair; leaving the DB row untouched"
                );
                report.unparseable_words_json.push(transcript_id);
            }
        }
    }

    Ok(())
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
    use super::*;

    async fn test_db() -> hypr_db_core::Db {
        let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
        hypr_db_app::prepare_schema(&db).await.unwrap();
        db
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

    async fn words_json_for(pool: &SqlitePool, transcript_id: &str) -> String {
        sqlx::query_scalar("SELECT words_json FROM transcripts WHERE id = ?")
            .bind(transcript_id)
            .fetch_one(pool)
            .await
            .unwrap()
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
    async fn first_run_repairs_legacy_words_json_in_the_db_and_writes_marker() {
        let db = test_db().await;
        let vault = tempfile::tempdir().unwrap();
        seed_session_and_transcript(
            db.pool(),
            "session-legacy-1",
            r#"[{"id":"w1","text":"hello","start_ms":"0","end_ms":"500","channel":"0"}]"#,
        )
        .await;

        let report = super::run_once(db.pool(), vault.path()).await.unwrap();

        assert!(!report.skipped_marker_present);
        assert_eq!(report.repaired_words_json, 1);
        assert!(report.unparseable_words_json.is_empty());

        let marker = vault.path().join(MARKER_FILENAME);
        assert!(marker.is_file(), "marker file must be written");

        let repaired = words_json_for(db.pool(), "session-legacy-1-transcript").await;
        assert!(
            serde_json::from_str::<Vec<hypr_fs_format::TranscriptWord>>(&repaired).is_ok(),
            "the DB row itself must now strict-parse"
        );
    }

    #[tokio::test]
    async fn marker_already_present_short_circuits_and_leaves_the_row_untouched() {
        let db = test_db().await;
        let vault = tempfile::tempdir().unwrap();
        seed_session_and_transcript(
            db.pool(),
            "session-legacy-2",
            r#"[{"text":"hi","start_ms":"0","end_ms":"100","channel":"0"}]"#,
        )
        .await;
        std::fs::write(vault.path().join(MARKER_FILENAME), MARKER_CONTENT).unwrap();

        let report = super::run_once(db.pool(), vault.path()).await.unwrap();

        assert_eq!(
            report,
            MigrateReport {
                skipped_marker_present: true,
                ..Default::default()
            }
        );
        assert_eq!(
            words_json_for(db.pool(), "session-legacy-2-transcript").await,
            r#"[{"text":"hi","start_ms":"0","end_ms":"100","channel":"0"}]"#,
            "a repairable row must stay untouched once the marker exists"
        );
    }

    #[tokio::test]
    async fn second_run_is_a_no_op() {
        let db = test_db().await;
        let vault = tempfile::tempdir().unwrap();
        seed_session_and_transcript(
            db.pool(),
            "session-legacy-3",
            r#"[{"text":"hi","start_ms":"0","end_ms":"100","channel":"0"}]"#,
        )
        .await;

        super::run_once(db.pool(), vault.path()).await.unwrap();
        let report = super::run_once(db.pool(), vault.path()).await.unwrap();

        assert!(report.skipped_marker_present);
        assert_eq!(report.repaired_words_json, 0);
    }

    #[tokio::test]
    async fn an_unparseable_row_is_reported_left_untouched_and_does_not_block_the_marker() {
        let db = test_db().await;
        let vault = tempfile::tempdir().unwrap();
        seed_session_and_transcript(db.pool(), "session-broken", "not valid json words at all")
            .await;
        seed_session_and_transcript(
            db.pool(),
            "session-repairable",
            r#"[{"text":"fine","start_ms":"0","end_ms":"10","channel":"0"}]"#,
        )
        .await;

        let report = super::run_once(db.pool(), vault.path()).await.unwrap();

        assert!(!report.skipped_marker_present);
        assert_eq!(
            report.unparseable_words_json,
            vec!["session-broken-transcript".to_string()]
        );
        assert_eq!(
            report.repaired_words_json, 1,
            "one bad row must not block another row's repair"
        );

        assert_eq!(
            words_json_for(db.pool(), "session-broken-transcript").await,
            "not valid json words at all"
        );
        assert!(
            vault.path().join(MARKER_FILENAME).is_file(),
            "one bad row must not block the marker"
        );
    }
}
