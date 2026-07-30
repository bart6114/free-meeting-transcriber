use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::{Args, Error, Result};

pub fn open(args: &Args) -> Result<PathBuf> {
    let path = resolve_path(args)?;
    if !path.is_dir() {
        return Err(Error::VaultNotFound(path));
    }
    Ok(path)
}

pub(crate) fn resolve_path(args: &Args) -> Result<PathBuf> {
    if let Some(path) = &args.vault_path {
        return Ok(path.clone());
    }
    if let Some(base) = &args.base {
        return Ok(base.clone());
    }

    let data_dir = dirs::data_dir()
        .ok_or_else(|| Error::operation("resolve vault path", "data directory is unavailable"))?;
    Ok(resolve_default_path(&data_dir))
}

fn resolve_default_path(data_dir: &Path) -> PathBuf {
    let command_name = std::env::args_os()
        .next()
        .and_then(|path| Path::new(&path).file_name().map(|name| name.to_owned()));
    resolve_default_path_for_command(data_dir, command_name.as_deref())
}

fn resolve_default_path_for_command(data_dir: &Path, command_name: Option<&OsStr>) -> PathBuf {
    let channel_identifier = match command_name.and_then(OsStr::to_str) {
        Some("fmtr-dev") => Some("org.freemeetingtranscriber.dev"),
        Some("fmtr-staging") => Some("org.freemeetingtranscriber.staging"),
        _ => None,
    };
    let base = match channel_identifier {
        Some(identifier) => data_dir.join(identifier),
        None => data_dir.join("free-meeting-transcriber"),
    };
    apply_vault_redirect(base)
}

/// The desktop app keeps its vault in the application-data directory by default, but a user
/// can relocate it; the app then records the absolute vault path under `vault_path` in the
/// app-data directory's `global.json`. Follow that redirect so the CLI reads the same vault
/// the desktop app uses.
fn apply_vault_redirect(base: PathBuf) -> PathBuf {
    let Ok(raw) = std::fs::read_to_string(base.join("global.json")) else {
        return base;
    };
    let Ok(config) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return base;
    };
    match config.get("vault_path").and_then(|value| value.as_str()) {
        Some(vault_path) if Path::new(vault_path).is_absolute() => PathBuf::from(vault_path),
        _ => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_path_targets_the_app_data_vault() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_default_path_for_command(dir.path(), Some(OsStr::new("fmtr"))),
            dir.path().join("free-meeting-transcriber")
        );
    }

    #[test]
    fn channel_commands_target_their_channel_vault() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_default_path_for_command(dir.path(), Some(OsStr::new("fmtr-dev"))),
            dir.path().join("org.freemeetingtranscriber.dev")
        );
        assert_eq!(
            resolve_default_path_for_command(dir.path(), Some(OsStr::new("fmtr-staging"))),
            dir.path().join("org.freemeetingtranscriber.staging")
        );
    }

    #[test]
    fn default_path_follows_the_global_json_vault_redirect() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("free-meeting-transcriber");
        let vault = dir.path().join("my-vault");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            base.join("global.json"),
            serde_json::json!({ "vault_path": vault.to_string_lossy() }).to_string(),
        )
        .unwrap();

        assert_eq!(
            resolve_default_path_for_command(dir.path(), Some(OsStr::new("fmtr"))),
            vault
        );
    }

    #[test]
    fn relative_or_malformed_redirects_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("free-meeting-transcriber");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            base.join("global.json"),
            serde_json::json!({ "vault_path": "relative/vault" }).to_string(),
        )
        .unwrap();
        assert_eq!(
            resolve_default_path_for_command(dir.path(), Some(OsStr::new("fmtr"))),
            base
        );

        std::fs::write(base.join("global.json"), "{ invalid").unwrap();
        assert_eq!(
            resolve_default_path_for_command(dir.path(), Some(OsStr::new("fmtr"))),
            base
        );
    }
}
