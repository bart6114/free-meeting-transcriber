mod disk;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ractor::{Actor, ActorName, ActorProcessingErr, ActorRef};

pub enum RecMsg {
    AudioSingle(Arc<[f32]>),
    AudioDual(Arc<[f32]>, Arc<[f32]>),
}

pub struct RecArgs {
    pub vault_dir: PathBuf,
    pub session_id: String,
}

pub struct RecState {
    sink: RecorderSink,
}

enum RecorderSink {
    Disk(disk::DiskSink),
}

pub struct RecorderActor;

impl Default for RecorderActor {
    fn default() -> Self {
        Self::new()
    }
}

impl RecorderActor {
    pub fn new() -> Self {
        Self
    }

    pub fn name() -> ActorName {
        "recorder_actor".into()
    }
}

#[ractor::async_trait]
impl Actor for RecorderActor {
    type Msg = RecMsg;
    type State = RecState;
    type Arguments = RecArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let session_dir = find_session_dir(&args.vault_dir, &args.session_id);
        std::fs::create_dir_all(&session_dir)?;

        Ok(RecState {
            sink: RecorderSink::Disk(disk::create_disk_sink(&session_dir)?),
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        st: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match (&mut st.sink, msg) {
            (RecorderSink::Disk(sink), RecMsg::AudioSingle(samples)) => {
                disk::write_single(sink, &samples)?;
            }
            (RecorderSink::Disk(sink), RecMsg::AudioDual(mic, spk)) => {
                disk::write_dual(sink, &mic, &spk)?;
            }
        }

        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        st: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match &mut st.sink {
            RecorderSink::Disk(sink) => {
                disk::finalize_disk_sink(sink)?;
            }
        }

        Ok(())
    }
}

/// Resolve a session's physical directory by `_meta.json.id`. When the id resolves
/// nowhere (or the lookup fails), fall back to the legacy `sessions/<id>` path:
/// recording into a not-yet-created session must still persist somewhere the store's
/// ghost-session handling will pick up.
pub fn find_session_dir(vault_base: &Path, session_id: &str) -> PathBuf {
    match hypr_vault_read::find_session(vault_base, session_id) {
        Ok(Some((location, _))) => vault_base.join(location.relative_dir),
        Ok(None) => legacy_session_dir(vault_base, session_id),
        Err(error) => {
            tracing::warn!(
                fmtr.session.id = %session_id,
                error.message = %error,
                "session_lookup_failed_using_legacy_dir"
            );
            legacy_session_dir(vault_base, session_id)
        }
    }
}

fn legacy_session_dir(vault_base: &Path, session_id: &str) -> PathBuf {
    vault_base
        .join(hypr_vault_read::paths::sessions_root())
        .join(session_id)
}

pub fn resolve_final_audio_path(vault_base: &Path, session_id: &str) -> Option<PathBuf> {
    let session_dir = find_session_dir(vault_base, session_id);
    let mp3_path = session_dir.join("audio.mp3");
    if mp3_path.exists() {
        return Some(mp3_path);
    }

    let wav_path = session_dir.join("audio.wav");
    if wav_path.exists() {
        return Some(wav_path);
    }

    let ogg_path = session_dir.join("audio.ogg");
    if ogg_path.exists() {
        return Some(ogg_path);
    }

    None
}

fn into_actor_err<E>(err: E) -> ActorProcessingErr
where
    E: std::error::Error + Send + Sync + 'static,
{
    Box::new(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    const UUID_1: &str = "550e8400-e29b-41d4-a716-446655440000";

    fn seed_session_at(vault: &Path, relative_dir: &str, id: &str) {
        let dir = vault.join(relative_dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("_meta.json"),
            serde_json::json!({
                "id": id,
                "title": "Test",
                "started_at": null,
                "ended_at": null,
                "created_at": "2026-03-20T00:00:00Z",
                "tags": [],
            })
            .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn resolves_readable_directory_by_meta_id() {
        let vault = tempfile::tempdir().unwrap();
        let readable = "sessions/2026-03-20 — Test — abc123";
        seed_session_at(vault.path(), readable, UUID_1);

        assert_eq!(
            find_session_dir(vault.path(), UUID_1),
            vault.path().join(readable)
        );
    }

    #[test]
    fn resolves_legacy_uuid_directory() {
        let vault = tempfile::tempdir().unwrap();
        let legacy = format!("sessions/{UUID_1}");
        seed_session_at(vault.path(), &legacy, UUID_1);

        assert_eq!(
            find_session_dir(vault.path(), UUID_1),
            vault.path().join(legacy)
        );
    }

    #[test]
    fn falls_back_to_legacy_path_when_session_resolves_nowhere() {
        let vault = tempfile::tempdir().unwrap();

        assert_eq!(
            find_session_dir(vault.path(), UUID_1),
            vault.path().join("sessions").join(UUID_1)
        );
    }

    #[test]
    fn resolves_readable_directory_nested_in_personal_folder() {
        let vault = tempfile::tempdir().unwrap();
        let nested = "sessions/Work/2026-03-20 — Planning — abc123";
        seed_session_at(vault.path(), nested, UUID_1);

        assert_eq!(
            find_session_dir(vault.path(), UUID_1),
            vault.path().join(nested)
        );
    }

    #[test]
    fn final_audio_path_lands_in_readable_directory() {
        let vault = tempfile::tempdir().unwrap();
        let readable = "sessions/2026-03-20 — Test — abc123";
        seed_session_at(vault.path(), readable, UUID_1);
        std::fs::write(vault.path().join(readable).join("audio.mp3"), b"mp3").unwrap();

        assert_eq!(
            resolve_final_audio_path(vault.path(), UUID_1),
            Some(vault.path().join(readable).join("audio.mp3"))
        );
    }

    #[test]
    fn final_audio_path_still_finds_legacy_uuid_directory() {
        let vault = tempfile::tempdir().unwrap();
        let legacy = format!("sessions/{UUID_1}");
        seed_session_at(vault.path(), &legacy, UUID_1);
        std::fs::write(vault.path().join(&legacy).join("audio.wav"), b"wav").unwrap();

        assert_eq!(
            resolve_final_audio_path(vault.path(), UUID_1),
            Some(vault.path().join(legacy).join("audio.wav"))
        );
    }
}
