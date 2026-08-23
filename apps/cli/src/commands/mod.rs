pub mod doctor;
pub mod import;
pub mod meetings;
pub mod tags;
pub mod transcribe;

use std::path::Path;

use crate::{Error, Result};
use hypr_vault_write::{SessionMeta, SessionStore};

/// Caller-provided metadata for a new session. Timestamps are already
/// normalized to millisecond UTC RFC 3339 by the clap value parser; a missing
/// `created_at` falls back to now, and tags land in `_meta.json` verbatim.
#[derive(Debug, Default)]
pub(crate) struct NewSessionOptions {
    pub created_at: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub tags: Vec<String>,
    pub author: Option<String>,
    pub skill: Option<String>,
}

/// Create a session the same way the desktop frontend does: a `crypto.randomUUID()`-style
/// id plus a full `_meta.json`. A collision is practically impossible, but never clobber
/// an existing session: retry a few times, then give up rather than overwrite.
pub(crate) async fn create_session(
    vault: &Path,
    store: &SessionStore,
    action: &'static str,
    title: String,
    options: NewSessionOptions,
) -> Result<SessionMeta> {
    let mut session_id = None;
    for _ in 0..5 {
        let candidate = uuid::Uuid::new_v4().to_string();
        // Only the legacy `sessions/<id>` path is probed (one stat, O(1) in
        // vault size): a readable-named directory can only claim a just-minted
        // v4 UUID via RNG collision, so the full logical-occupancy scan this
        // used to pay made every creation O(vault) — minutes on a network
        // mount — to defend against a ~2^-122 event. The store's creation path
        // below still refuses occupied directory names.
        let probe = vault
            .join(hypr_vault_read::paths::sessions_root())
            .join(&candidate);
        let occupied = tokio::task::spawn_blocking(move || probe.exists())
            .await
            .map_err(|error| Error::operation(action, error.to_string()))?;
        if !occupied {
            session_id = Some(candidate);
            break;
        }
    }
    let session_id = session_id
        .ok_or_else(|| Error::operation(action, "could not generate an unused meeting id"))?;

    // Millisecond RFC3339 UTC, matching the desktop's `new Date().toISOString()`.
    let created_at = options
        .created_at
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    let meta = SessionMeta {
        id: session_id,
        title,
        started_at: options.started_at,
        ended_at: options.ended_at,
        created_at,
        tags: options.tags,
        tracking_id: None,
        folder: None,
        author: options.author,
        skill: options.skill,
        extra: Default::default(),
    };
    store
        .create_session_meta(&meta)
        .await
        .map_err(|error| Error::operation(action, error.to_string()))?;
    Ok(meta)
}
