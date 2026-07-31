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
            return;
        };

        // Stamped here rather than inside the spawned task: the event marks the actual
        // lifecycle moment, the store write merely persists it.
        let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        tauri::async_runtime::spawn(async move {
            let result = if is_end {
                store.mark_recording_ended(&session_id, &at).await
            } else {
                store.mark_recording_started(&session_id, &at).await
            };
            if let Err(error) = result {
                tracing::warn!(
                    %session_id,
                    %error,
                    "failed to stamp recording timestamp in _meta.json"
                );
            }
        });
    });
}
