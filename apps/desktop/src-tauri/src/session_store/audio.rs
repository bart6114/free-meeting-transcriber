use std::path::{Path, PathBuf};

use super::{SessionStore, StoreError, paths, validate_session_id};

/// Moves `source` to `dest` (assumed to not yet exist; `dest`'s parent must already exist).
/// Prefers `rename` (atomic, no data-duplication window); falls back to copy+delete when
/// `rename` fails (typically EXDEV -- source and dest on different volumes, which
/// `std::fs::rename` can't cross). Returns whether the fallback path was taken, so callers
/// (and tests) can tell which branch actually ran. `rename` is injected so the fallback branch
/// can be exercised deterministically in tests without needing a real cross-device setup.
fn move_or_copy_delete(
    source: &Path,
    dest: &Path,
    rename: fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<bool, StoreError> {
    if rename(source, dest).is_ok() {
        return Ok(false);
    }

    std::fs::copy(source, dest)
        .map_err(|e| StoreError::Io(format!("failed to copy audio file: {}", e)))?;
    std::fs::remove_file(source).map_err(|e| {
        StoreError::Io(format!(
            "failed to remove source audio file after copy: {}",
            e
        ))
    })?;
    Ok(true)
}

/// `std::fs::rename` is generic over `AsRef<Path>`, so it doesn't coerce directly to the
/// `for<'a, 'b> fn(&'a Path, &'b Path) -> io::Result<()>` pointer type `move_or_copy_delete`
/// expects (a specific monomorphization isn't the same as being generic over all lifetimes) --
/// this plain wrapper has no such generics and coerces cleanly.
fn plain_rename(source: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::rename(source, dest)
}

impl SessionStore {
    /// Moves a finished recording's audio file into `sessions/<id>/audio/<filename>`. Prefers
    /// `std::fs::rename` (atomic, no data-duplication window); falls back to copy+delete when
    /// the source and destination are on different volumes (rename can't cross devices).
    /// Returns the new path relative to the vault base.
    pub async fn store_audio(
        &self,
        session_id: &str,
        source_path: &str,
    ) -> Result<String, StoreError> {
        validate_session_id(session_id)?;
        let vault_base = self.vault_base.clone();
        let session_id = session_id.to_string();
        let source_path = PathBuf::from(source_path);

        tokio::task::spawn_blocking(move || -> Result<String, StoreError> {
            let file_name = source_path
                .file_name()
                .ok_or_else(|| StoreError::Io("source path has no file name".to_string()))?;

            let audio_dir_rel = paths::audio_dir(&session_id);
            let audio_dir_abs = vault_base.join(&audio_dir_rel);
            std::fs::create_dir_all(&audio_dir_abs)
                .map_err(|e| StoreError::Io(format!("failed to create audio directory: {}", e)))?;

            let dest_abs = audio_dir_abs.join(file_name);

            move_or_copy_delete(&source_path, &dest_abs, plain_rename)?;

            let dest_rel = audio_dir_rel.join(file_name);
            dest_rel
                .to_str()
                .map(|s| s.to_string())
                .ok_or_else(|| StoreError::Io("invalid destination path".to_string()))
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {}", e)))?
    }

    /// Lists audio file names (not full paths) under `sessions/<id>/audio/`, sorted. Missing
    /// directory -> empty list, matching the "nothing recorded yet" state rather than an error.
    pub async fn list_audio(&self, session_id: &str) -> Result<Vec<String>, StoreError> {
        validate_session_id(session_id)?;
        let vault_base = self.vault_base.clone();
        let session_id = session_id.to_string();

        tokio::task::spawn_blocking(move || -> Result<Vec<String>, StoreError> {
            let dir = vault_base.join(paths::audio_dir(&session_id));
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
                Err(e) => {
                    return Err(StoreError::Io(format!(
                        "failed to read audio directory: {}",
                        e
                    )));
                }
            };

            let mut names = Vec::new();
            for entry in entries {
                let entry = entry
                    .map_err(|e| StoreError::Io(format!("failed to read dir entry: {}", e)))?;
                if entry.path().is_file()
                    && let Some(name) = entry.file_name().to_str()
                {
                    names.push(name.to_string());
                }
            }
            names.sort();
            Ok(names)
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {}", e)))?
    }

    /// Permanently deletes one audio file under `sessions/<id>/audio/` (retention cleanup --
    /// unlike `delete_session`, this is not undo-able, matching the old retention behavior it
    /// replaces). Missing file is a no-op, not an error.
    pub async fn delete_audio(&self, session_id: &str, filename: &str) -> Result<(), StoreError> {
        validate_session_id(session_id)?;
        let vault_base = self.vault_base.clone();
        let session_id = session_id.to_string();
        let filename = filename.to_string();

        tokio::task::spawn_blocking(move || -> Result<(), StoreError> {
            // `filename` must be a bare file name: callers only ever pass one back from
            // `list_audio`, but reject path separators/traversal defensively so a bad
            // filename can't escape the audio directory via this command boundary.
            if filename.is_empty() || filename.contains(['/', '\\']) || filename == ".." {
                return Err(StoreError::Io(format!(
                    "invalid audio filename: {}",
                    filename
                )));
            }

            let path = vault_base
                .join(paths::audio_dir(&session_id))
                .join(&filename);
            match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(StoreError::Io(format!(
                    "failed to delete audio file: {}",
                    e
                ))),
            }
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
        let store = SessionStore::new(vault);
        (store, temp)
    }

    #[tokio::test]
    async fn store_audio_moves_file_into_session_audio_dir() {
        let (store, vault) = test_store().await;
        let source = vault.path().join("recording.wav");
        std::fs::write(&source, b"wav-bytes").unwrap();

        let relative = store
            .store_audio("s1", source.to_str().unwrap())
            .await
            .unwrap();

        assert_eq!(relative, "sessions/s1/audio/recording.wav");
        assert!(vault.path().join(&relative).is_file());
        assert!(!source.exists(), "source file must be moved, not copied");
        assert_eq!(
            std::fs::read(vault.path().join(&relative)).unwrap(),
            b"wav-bytes"
        );
    }

    #[tokio::test]
    async fn store_audio_uses_plain_rename_on_the_happy_path() {
        let (store, vault) = test_store().await;
        let source = vault.path().join("nested").join("take.wav");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, b"content").unwrap();

        let relative = store
            .store_audio("s1", source.to_str().unwrap())
            .await
            .unwrap();

        assert_eq!(
            std::fs::read(vault.path().join(&relative)).unwrap(),
            b"content"
        );
        assert!(!source.exists());
    }

    /// REGRESSION (reviewer-found minor): the previous version of this test asserted only the
    /// end state (file moved, source gone), which the plain-rename path already satisfies on a
    /// single filesystem -- it never actually forced `rename` to fail, so the copy+delete
    /// fallback branch went unexercised. `move_or_copy_delete` takes `rename` as a parameter
    /// specifically so a failure can be injected deterministically here, without needing a
    /// real cross-device mount.
    #[test]
    fn move_or_copy_delete_falls_back_to_copy_and_removes_the_source_when_rename_fails() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.wav");
        let dest = temp.path().join("dest.wav");
        std::fs::write(&source, b"fallback-content").unwrap();

        let used_fallback = move_or_copy_delete(&source, &dest, |_, _| {
            Err(std::io::Error::other("forced rename failure"))
        })
        .unwrap();

        assert!(used_fallback, "must report that the fallback branch ran");
        assert_eq!(std::fs::read(&dest).unwrap(), b"fallback-content");
        assert!(!source.exists(), "source must be removed after the copy");
    }

    #[test]
    fn move_or_copy_delete_reports_no_fallback_when_rename_succeeds() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.wav");
        let dest = temp.path().join("dest.wav");
        std::fs::write(&source, b"renamed-content").unwrap();

        let used_fallback = move_or_copy_delete(&source, &dest, plain_rename).unwrap();

        assert!(!used_fallback, "must report that rename succeeded directly");
        assert_eq!(std::fs::read(&dest).unwrap(), b"renamed-content");
        assert!(!source.exists());
    }

    #[tokio::test]
    async fn list_audio_returns_empty_for_missing_session() {
        let (store, _vault) = test_store().await;
        assert_eq!(
            store.list_audio("nonexistent").await.unwrap(),
            Vec::<String>::new()
        );
    }

    #[tokio::test]
    async fn list_audio_returns_sorted_file_names() {
        let (store, vault) = test_store().await;
        let audio_dir = vault.path().join("sessions/s1/audio");
        std::fs::create_dir_all(&audio_dir).unwrap();
        std::fs::write(audio_dir.join("b.wav"), b"").unwrap();
        std::fs::write(audio_dir.join("a.wav"), b"").unwrap();

        assert_eq!(
            store.list_audio("s1").await.unwrap(),
            vec!["a.wav".to_string(), "b.wav".to_string()]
        );
    }

    #[tokio::test]
    async fn delete_audio_removes_file() {
        let (store, vault) = test_store().await;
        let audio_dir = vault.path().join("sessions/s1/audio");
        std::fs::create_dir_all(&audio_dir).unwrap();
        std::fs::write(audio_dir.join("take.wav"), b"").unwrap();

        store.delete_audio("s1", "take.wav").await.unwrap();

        assert!(!audio_dir.join("take.wav").exists());
    }

    #[tokio::test]
    async fn delete_audio_missing_file_is_a_noop() {
        let (store, _vault) = test_store().await;
        store.delete_audio("s1", "never-existed.wav").await.unwrap();
    }

    #[tokio::test]
    async fn delete_audio_rejects_path_traversal() {
        let (store, _vault) = test_store().await;
        let result = store.delete_audio("s1", "../../etc/passwd").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StoreError::Io(_)));
    }
}
