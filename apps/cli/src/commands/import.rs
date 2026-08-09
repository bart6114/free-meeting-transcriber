use std::path::{Path, PathBuf};

use crate::{Error, Result, output};
use hypr_vault_write::SessionStore;

/// The formats the desktop app accepts in its import dialog. `normalize_file`
/// re-encodes all of them to the vault's 16 kHz `audio.mp3`.
const SUPPORTED_EXTENSIONS: [&str; 8] = ["wav", "mp3", "ogg", "mp4", "m4a", "flac", "webm", "aac"];

pub async fn run(vault: &Path, file: PathBuf, title: Option<String>, json: bool) -> Result<()> {
    // Validate the input before touching the vault, so a bad file creates nothing.
    let extension = file
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();
    if !SUPPORTED_EXTENSIONS.contains(&extension.as_str()) {
        return Err(Error::operation(
            "import audio",
            format!(
                "unsupported audio format for {}; supported extensions: {}",
                file.display(),
                SUPPORTED_EXTENSIONS.join(", ")
            ),
        ));
    }
    if !file.is_file() {
        return Err(Error::NotFound(format!("audio file {}", file.display())));
    }

    let title = title.unwrap_or_else(|| {
        file.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    });

    let store = SessionStore::new(vault.to_path_buf());
    let meta = super::create_session(vault, &store, "import audio", title).await?;
    let session_id = meta.id.clone();

    // Same layout and conversion as the desktop import path (fs-sync-core's
    // `import_to_session`): normalize to 16 kHz MP3 via a temp file, then move
    // atomically into place.
    let session_dir = vault.join("sessions").join(&session_id);
    let tmp_path = session_dir.join("audio.mp3.tmp");
    let target_path = session_dir.join("audio.mp3");
    let source_path = file.clone();
    let audio_path = tokio::task::spawn_blocking(move || {
        hypr_audio_norm::normalize_file(
            &source_path,
            &tmp_path,
            &target_path,
            None,
            None::<fn(f64)>,
        )
    })
    .await
    .map_err(|error| Error::operation("import audio", error.to_string()))?
    .map_err(|error| {
        // The session already exists at this point; name it so the partial
        // import is identifiable instead of an anonymous orphan.
        Error::operation(
            "import audio",
            format!("meeting {session_id} was created, but converting its audio failed: {error}"),
        )
    })?;

    let rendered = if json {
        output::json(
            "import",
            &serde_json::json!({
                "id": session_id,
                "title": meta.title,
                "created_at": meta.created_at,
                "audio": audio_path,
            }),
            None,
        )?
    } else {
        session_id
    };
    output::emit(&rendered);
    Ok(())
}
