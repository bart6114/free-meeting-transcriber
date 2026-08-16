use std::path::PathBuf;

use serde::Serialize;

use crate::{Args, Result, output, vault};

#[derive(Debug, Serialize)]
struct DoctorReport {
    cli_version: &'static str,
    ready: bool,
    vault: VaultReport,
}

#[derive(Debug, Serialize)]
struct VaultReport {
    path: PathBuf,
    exists: bool,
    is_directory: bool,
    sessions: Option<usize>,
    error: Option<String>,
}

pub fn run(args: &Args, json: bool) -> Result<bool> {
    let report = inspect(args)?;
    let rendered = if json {
        output::json("doctor", &report, None)?
    } else {
        render(&report)
    };
    output::emit(&rendered);
    Ok(report.ready)
}

fn inspect(args: &Args) -> Result<DoctorReport> {
    let path = vault::resolve_path(args)?;
    let exists = path.exists();
    let mut report = VaultReport {
        path: path.clone(),
        exists,
        is_directory: false,
        sessions: None,
        error: None,
    };

    if !exists {
        report.error = Some("vault directory does not exist".to_string());
    } else if !path.is_dir() {
        report.error = Some("vault path is not a directory".to_string());
    } else {
        report.is_directory = true;
        match hypr_vault_read::meta::list_session_metas(&path) {
            Ok(metas) => report.sessions = Some(metas.len()),
            Err(error) => report.error = Some(format!("vault scan failed: {error}")),
        }
    }

    Ok(DoctorReport {
        cli_version: env!("FMTR_VERSION"),
        ready: report.is_directory && report.sessions.is_some(),
        vault: report,
    })
}

fn render(report: &DoctorReport) -> String {
    let status = |value| if value { "yes" } else { "no" };
    let mut lines = vec![
        format!("fmtr CLI {}", report.cli_version),
        format!("Ready: {}", status(report.ready)),
        format!("Vault: {}", report.vault.path.display()),
        format!("Exists: {}", status(report.vault.exists)),
        format!("Directory: {}", status(report.vault.is_directory)),
    ];
    if let Some(sessions) = report.vault.sessions {
        lines.push(format!("Sessions: {sessions}"));
    }
    if let Some(error) = &report.vault.error {
        lines.push(format!("Issue: {error}"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use crate::cli::Command;

    use super::*;

    fn args(path: PathBuf) -> Args {
        Args {
            base: None,
            vault_path: Some(path),
            json: true,
            command: Command::Doctor,
        }
    }

    #[test]
    fn reports_missing_vault_as_not_ready() {
        let dir = tempfile::tempdir().unwrap();
        let report = inspect(&args(dir.path().join("missing-vault"))).unwrap();

        assert!(!report.ready);
        assert!(!report.vault.exists);
        assert_eq!(
            report.vault.error.as_deref(),
            Some("vault directory does not exist")
        );
    }

    #[test]
    fn reports_a_vault_file_path_as_not_ready() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault");
        std::fs::write(&path, "not a directory").unwrap();
        let report = inspect(&args(path)).unwrap();

        assert!(!report.ready);
        assert!(report.vault.exists);
        assert!(!report.vault.is_directory);
        assert_eq!(
            report.vault.error.as_deref(),
            Some("vault path is not a directory")
        );
    }

    #[test]
    fn reports_a_scannable_vault_as_ready() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("sessions/meeting-1");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(
            session_dir.join("_meta.json"),
            serde_json::json!({
                "id": "meeting-1",
                "title": "Planning",
                "started_at": null,
                "ended_at": null,
                "created_at": "2026-07-13T00:00:00Z",
                "tags": [],
            })
            .to_string(),
        )
        .unwrap();

        let report = inspect(&args(dir.path().to_path_buf())).unwrap();

        assert!(report.ready);
        assert!(report.vault.is_directory);
        assert_eq!(report.vault.sessions, Some(1));
        assert!(report.vault.error.is_none());
    }
}
