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
        // Occupancy is logical, not physical: an id is taken when any directory
        // claims it in `_meta.json`, wherever that directory lives and whatever
        // it is named. Ambiguous (duplicate claims) and corrupt legacy metadata
        // both count as taken — never risk clobbering either.
        let scan_vault = vault.to_path_buf();
        let scan_id = candidate.clone();
        let occupied = match tokio::task::spawn_blocking(move || {
            hypr_vault_read::find_session(&scan_vault, &scan_id)
        })
        .await
        .map_err(|error| Error::operation(action, error.to_string()))?
        {
            Ok(existing) => existing.is_some(),
            Err(
                hypr_vault_read::SessionLookupError::Ambiguous { .. }
                | hypr_vault_read::SessionLookupError::Corrupt { .. },
            ) => true,
            Err(error) => return Err(Error::operation(action, error.to_string())),
        };
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
        extra: Default::default(),
    };
    store
        .write_meta(&meta)
        .await
        .map_err(|error| Error::operation(action, error.to_string()))?;
    Ok(meta)
}
