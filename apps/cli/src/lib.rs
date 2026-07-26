#![forbid(unsafe_code)]

mod cli;
mod commands;
mod error;
mod mcp;
mod output;
mod vault;

pub use cli::Args;
pub use error::{Error, Result};
pub use output::JSON_SCHEMA_VERSION;

pub async fn run(args: Args) -> Result<u8> {
    if matches!(&args.command, cli::Command::Doctor) {
        let ready = commands::doctor::run(&args, args.json)?;
        return Ok(if ready { 0 } else { 1 });
    }

    let vault = vault::open(&args)?;

    match args.command {
        cli::Command::Doctor => unreachable!("doctor returns before opening the vault"),
        cli::Command::Meetings { command } => {
            commands::meetings::run(&vault, command, args.json).await?
        }
        cli::Command::Mcp => mcp::serve(vault).await?,
    }

    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn doctor_returns_nonzero_status_when_vault_is_not_ready() {
        let dir = tempfile::tempdir().unwrap();
        let status = run(Args {
            base: None,
            vault_path: Some(dir.path().join("missing-vault")),
            json: true,
            command: cli::Command::Doctor,
        })
        .await
        .unwrap();

        assert_eq!(status, 1);
    }

    #[tokio::test]
    async fn export_command_reads_an_existing_vault_without_writing_to_it() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("vault");
        let output_path = dir.path().join("meeting.md");
        let session_dir = vault.join("sessions/meeting-1");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(
            session_dir.join("_meta.json"),
            serde_json::json!({
                "id": "meeting-1",
                "title": "Planning",
                "started_at": "2026-07-13",
                "ended_at": null,
                "created_at": "2026-07-13T00:00:00Z",
                "tags": [],
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(session_dir.join("_memo.md"), "Decide the launch date.").unwrap();

        run(Args {
            base: None,
            vault_path: Some(vault),
            json: false,
            command: cli::Command::Meetings {
                command: cli::MeetingCommand::Export {
                    id: "meeting-1".to_string(),
                    format: cli::ExportFormat::Markdown,
                    output: Some(output_path.clone()),
                    force: false,
                },
            },
        })
        .await
        .unwrap();

        let exported = std::fs::read_to_string(output_path).unwrap();
        assert!(exported.contains("# Planning"));
        assert!(exported.contains("Decide the launch date."));
    }
}
