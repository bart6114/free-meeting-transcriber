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

    fn write_session(vault: &std::path::Path, id: &str, note: Option<&str>) {
        let session_dir = vault.join("sessions").join(id);
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(
            session_dir.join("_meta.json"),
            serde_json::json!({
                "id": id,
                "title": "Planning",
                "started_at": null,
                "ended_at": null,
                "created_at": "2026-07-13T00:00:00Z",
                "tags": [],
            })
            .to_string(),
        )
        .unwrap();
        if let Some(note) = note {
            std::fs::write(session_dir.join("_memo.md"), note).unwrap();
        }
    }

    #[tokio::test]
    async fn new_command_creates_meta_and_note_readable_via_the_read_path() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        let body_path = dir.path().join("body.md");
        std::fs::write(&body_path, "Decide the launch date.\n").unwrap();

        run(Args {
            base: None,
            vault_path: Some(vault.clone()),
            json: true,
            command: cli::Command::Meetings {
                command: cli::MeetingCommand::New {
                    title: "Kickoff".to_string(),
                    note: Some(body_path),
                },
            },
        })
        .await
        .unwrap();

        let sessions = std::fs::read_dir(vault.join("sessions"))
            .unwrap()
            .map(|entry| entry.unwrap())
            .collect::<Vec<_>>();
        assert_eq!(sessions.len(), 1);
        let id = sessions[0].file_name().into_string().unwrap();
        // Desktop id format: lowercase hyphenated UUID (crypto.randomUUID()).
        assert_eq!(id.len(), 36);
        assert_eq!(id.matches('-').count(), 4);
        assert_eq!(id, id.to_lowercase());

        let meta: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(sessions[0].path().join("_meta.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(meta["id"], id.as_str());
        assert_eq!(meta["title"], "Kickoff");
        assert_eq!(meta["tags"], serde_json::json!([]));
        // Desktop timestamp format: RFC3339 UTC with millisecond precision and Z.
        let created_at = meta["created_at"].as_str().unwrap();
        assert!(
            created_at.ends_with('Z') && created_at.len() == 24,
            "unexpected created_at format: {created_at}"
        );
        assert_eq!(
            std::fs::read_to_string(sessions[0].path().join("_memo.md")).unwrap(),
            "Decide the launch date.\n"
        );

        // The existing read path sees the new session.
        run(Args {
            base: None,
            vault_path: Some(vault),
            json: true,
            command: cli::Command::Meetings {
                command: cli::MeetingCommand::Note {
                    id,
                    kind: cli::DocumentKind::Note,
                    set: None,
                    append: None,
                },
            },
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn new_command_creates_nothing_when_the_note_source_is_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();

        let error = run(Args {
            base: None,
            vault_path: Some(vault.clone()),
            json: false,
            command: cli::Command::Meetings {
                command: cli::MeetingCommand::New {
                    title: "Kickoff".to_string(),
                    note: Some(dir.path().join("missing.md")),
                },
            },
        })
        .await
        .unwrap_err();

        assert_eq!(error.code(), "operation_failed");
        // The body is read before the vault is touched, so a bad --note path
        // must not leave a session behind.
        assert!(!vault.join("sessions").exists());
    }

    #[tokio::test]
    async fn note_set_replaces_and_append_concatenates_with_a_newline() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("vault");
        write_session(&vault, "meeting-1", Some("Original body"));
        let set_path = dir.path().join("set.md");
        std::fs::write(&set_path, "Replaced body").unwrap();
        let append_path = dir.path().join("append.md");
        std::fs::write(&append_path, "Appended line").unwrap();

        run(Args {
            base: None,
            vault_path: Some(vault.clone()),
            json: false,
            command: cli::Command::Meetings {
                command: cli::MeetingCommand::Note {
                    id: "meeting-1".to_string(),
                    kind: cli::DocumentKind::Note,
                    set: Some(set_path),
                    append: None,
                },
            },
        })
        .await
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(vault.join("sessions/meeting-1/_memo.md")).unwrap(),
            "Replaced body"
        );

        run(Args {
            base: None,
            vault_path: Some(vault.clone()),
            json: false,
            command: cli::Command::Meetings {
                command: cli::MeetingCommand::Note {
                    id: "meeting-1".to_string(),
                    kind: cli::DocumentKind::Note,
                    set: None,
                    append: Some(append_path),
                },
            },
        })
        .await
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(vault.join("sessions/meeting-1/_memo.md")).unwrap(),
            "Replaced body\nAppended line"
        );
    }

    #[tokio::test]
    async fn note_edit_fails_cleanly_when_the_session_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("vault");
        std::fs::create_dir_all(vault.join("sessions")).unwrap();
        let set_path = dir.path().join("set.md");
        std::fs::write(&set_path, "body").unwrap();

        let error = run(Args {
            base: None,
            vault_path: Some(vault.clone()),
            json: false,
            command: cli::Command::Meetings {
                command: cli::MeetingCommand::Note {
                    id: "missing".to_string(),
                    kind: cli::DocumentKind::Note,
                    set: Some(set_path),
                    append: None,
                },
            },
        })
        .await
        .unwrap_err();

        assert_eq!(error.code(), "not_found");
        assert_eq!(error.exit_code(), 2);
        assert!(!vault.join("sessions/missing").exists());
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
