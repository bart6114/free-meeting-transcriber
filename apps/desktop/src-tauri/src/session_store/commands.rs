use std::sync::Arc;

use tauri::{AppHandle, Manager};

use hypr_fs_format::TranscriptWithData;

use super::{RebuildReport, SessionMeta, SessionMetaPatch, SessionStore, TranscriptDelta};

/// Every command below is a thin wrapper: fetch the managed store, call the matching
/// `SessionStore` method, map `StoreError` to `String` for the IPC boundary. `SessionStore` is
/// `.manage()`d in `lib.rs`'s `setup()`; `try_state` (not `state`, which panics if unmanaged)
/// keeps a startup ordering mistake from crashing the whole IPC handler instead of surfacing a
/// normal `Err` the frontend already has to handle.
fn store<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Result<tauri::State<'_, Arc<SessionStore>>, String> {
    app.try_state::<Arc<SessionStore>>()
        .ok_or_else(|| "session store is not initialized".to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn session_write_meta<R: tauri::Runtime>(
    app: AppHandle<R>,
    meta: SessionMeta,
) -> Result<(), String> {
    store(&app)?
        .write_meta(&meta)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn session_update_meta<R: tauri::Runtime>(
    app: AppHandle<R>,
    session_id: String,
    patch: SessionMetaPatch,
) -> Result<(), String> {
    store(&app)?
        .update_meta(&session_id, patch)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn session_write_note<R: tauri::Runtime>(
    app: AppHandle<R>,
    session_id: String,
    markdown: String,
) -> Result<(), String> {
    store(&app)?
        .write_note(&session_id, &markdown)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn session_read_note<R: tauri::Runtime>(
    app: AppHandle<R>,
    session_id: String,
) -> Result<Option<String>, String> {
    store(&app)?
        .read_note(&session_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn session_write_document<R: tauri::Runtime>(
    app: AppHandle<R>,
    session_id: String,
    kind: String,
    markdown: String,
) -> Result<(), String> {
    store(&app)?
        .write_document(&session_id, &kind, &markdown)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn session_append_transcript<R: tauri::Runtime>(
    app: AppHandle<R>,
    session_id: String,
    delta: TranscriptDelta,
) -> Result<(), String> {
    store(&app)?
        .append_transcript(&session_id, delta)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn session_flush_transcript<R: tauri::Runtime>(
    app: AppHandle<R>,
    session_id: String,
) -> Result<(), String> {
    store(&app)?
        .flush_transcript(&session_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn session_write_transcript<R: tauri::Runtime>(
    app: AppHandle<R>,
    session_id: String,
    transcript: TranscriptWithData,
) -> Result<(), String> {
    store(&app)?
        .write_transcript(&session_id, transcript)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn session_delete<R: tauri::Runtime>(
    app: AppHandle<R>,
    session_id: String,
) -> Result<(), String> {
    store(&app)?
        .delete_session(&session_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn session_restore<R: tauri::Runtime>(
    app: AppHandle<R>,
    session_id: String,
) -> Result<bool, String> {
    store(&app)?
        .restore_session(&session_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn session_rebuild_index<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<RebuildReport, String> {
    store(&app)?
        .rebuild_index()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn session_store_audio<R: tauri::Runtime>(
    app: AppHandle<R>,
    session_id: String,
    source_path: String,
) -> Result<String, String> {
    store(&app)?
        .store_audio(&session_id, &source_path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn session_list_audio<R: tauri::Runtime>(
    app: AppHandle<R>,
    session_id: String,
) -> Result<Vec<String>, String> {
    store(&app)?
        .list_audio(&session_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn session_delete_audio<R: tauri::Runtime>(
    app: AppHandle<R>,
    session_id: String,
    filename: String,
) -> Result<(), String> {
    store(&app)?
        .delete_audio(&session_id, &filename)
        .await
        .map_err(|e| e.to_string())
}
