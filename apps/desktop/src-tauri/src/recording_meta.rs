//! Stamps `_meta.json`'s `started_at`/`ended_at` from the capture lifecycle. Sessions are
//! created with both null and no frontend code ever patches them, so without this bridge
//! every real recording kept null timestamps forever. Riding the same
//! `CaptureLifecycleEvent` the frontend consumes keeps the file write on the Rust side:
//! `Started` fires when capture actually begins, `Stopped` when the session supervisor
//! winds down -- including after an abnormal supervisor exit, which also ends there.

use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tauri_plugin_transcription::CaptureLifecycleEvent;
use tauri_specta::Event;

use crate::session_store::SessionStore;

/// Emitted after every `Stopped`-driven metadata attempt has finished -- including
/// the missing-store and failed-write branches. It means "the end-of-recording
/// meta stamp (and any provisional directory rename it triggered) is no longer in
/// flight", not that it succeeded: the frontend waits for it before resolving
/// `resource_dir` for the post-stop hook, so the hook can never receive a path the
/// pending rename is about to move.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct RecordingMetaSettled {
    pub session_id: String,
    pub succeeded: bool,
}

pub fn spawn(app: AppHandle) {
    let handle = app.clone();
    CaptureLifecycleEvent::listen(&app, move |event| {
        let (session_id, is_end) = match event.payload {
            CaptureLifecycleEvent::Started { session_id, .. } => (session_id, false),
            CaptureLifecycleEvent::Stopped { session_id, .. } => (session_id, true),
            CaptureLifecycleEvent::Finalizing { .. } => return,
        };

        let Some(store) = handle
            .try_state::<Arc<SessionStore>>()
            .map(|state| state.inner().clone())
        else {
            tracing::warn!(
                %session_id,
                "session store is not managed; recording timestamps will stay null"
            );
            // Nothing will be stamped or renamed, and a waiter must not hang on it.
            if is_end {
                let _ = RecordingMetaSettled {
                    session_id,
                    succeeded: false,
                }
                .emit(&handle);
            }
            return;
        };

        // Registered synchronously, before the async stamp is even scheduled: the
        // provisional-directory rename deferral must be in force the moment capture
        // starts, not whenever the spawned task gets around to running -- a title
        // typed right after hitting record must not rename the directory the
        // recorder is writing into.
        if !is_end {
            store.note_recording_active(&session_id);
        }

        // Stamped here rather than inside the spawned task: the event marks the actual
        // lifecycle moment, the store write merely persists it.
        let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let emit_handle = handle.clone();
        tauri::async_runtime::spawn(async move {
            let result = if is_end {
                store.mark_recording_ended(&session_id, &at).await
            } else {
                store.mark_recording_started(&session_id, &at).await
            };
            if let Err(error) = &result {
                tracing::warn!(
                    %session_id,
                    %error,
                    "failed to stamp recording timestamp in _meta.json"
                );
            }
            if is_end {
                let _ = RecordingMetaSettled {
                    session_id,
                    succeeded: result.is_ok(),
                }
                .emit(&emit_handle);
            }
        });
    });
}
