use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use hypr_agent_access::{DEFAULT_SEARCH_LIMIT, MAX_SEARCH_LIMIT, SearchKind};

#[derive(Debug, Parser)]
#[command(
    name = "fmtr",
    version,
    about = "Query and edit local Free Meeting Transcriber meeting data"
)]
pub struct Args {
    #[arg(
        long,
        global = true,
        env = "FMTR_BASE",
        hide_env_values = true,
        value_name = "DIR"
    )]
    pub base: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        env = "FMTR_VAULT_PATH",
        hide_env_values = true,
        value_name = "DIR"
    )]
    pub vault_path: Option<PathBuf>,

    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Check the local CLI and vault access without changing data
    Doctor,
    /// Browse, create, edit, and export meetings
    Meetings {
        #[command(subcommand)]
        command: MeetingCommand,
    },
    /// Import an audio file into a new meeting and print its id
    Import {
        /// Audio file to import (wav, mp3, ogg, mp4, m4a, flac, webm, or aac)
        file: PathBuf,
        #[arg(long, help = "Title for the new meeting; defaults to the file name")]
        title: Option<String>,
        #[arg(
            long,
            help = "Transcribe the audio with the configured on-device model after importing"
        )]
        transcribe: bool,
    },
    /// Transcribe a meeting's audio with the configured on-device model
    Transcribe {
        /// Meeting id whose audio should be transcribed
        id: String,
    },
    /// Run the read-only Free Meeting Transcriber MCP server over stdio
    Mcp,
}

#[derive(Debug, Subcommand)]
pub enum MeetingCommand {
    /// List meetings, optionally filtered by text
    List {
        #[arg(short, long)]
        query: Option<String>,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=200), help = "Maximum results (1-200)")]
        limit: u32,
        #[arg(long, default_value_t = 0, help = "Number of results to skip")]
        offset: u32,
    },
    /// Search across meeting titles, notes, summaries, and transcripts
    #[command(group = clap::ArgGroup::new("criteria").required(true).multiple(true).args(["query", "speaker"]))]
    Search {
        /// Case-insensitive terms that must all occur
        query: Option<String>,
        #[arg(
            long,
            help = "Person id or name substring; limits hits to meetings where that person spoke"
        )]
        speaker: Option<String>,
        #[arg(
            long,
            value_enum,
            help = "Restrict to a source; repeatable, defaults to all"
        )]
        kind: Vec<SearchKindArg>,
        #[arg(long, default_value_t = DEFAULT_SEARCH_LIMIT, value_parser = clap::value_parser!(u32).range(1..=MAX_SEARCH_LIMIT as i64), help = "Maximum hits (1-50)")]
        limit: u32,
        #[arg(long, default_value_t = 0, help = "Number of hits to skip")]
        offset: u32,
    },
    /// Show meeting metadata, notes, summaries, and action items
    Get { id: String },
    /// Create a meeting note and print its id
    New {
        #[arg(long, help = "Title for the new meeting")]
        title: String,
        #[arg(
            long,
            value_name = "FILE",
            help = "Initial note body read from FILE, or '-' for stdin"
        )]
        note: Option<PathBuf>,
    },
    /// Show the note or generated summaries for a meeting, or edit the note
    Note {
        id: String,
        #[arg(long, value_enum, default_value_t = DocumentKind::Note, conflicts_with_all = ["set", "append"])]
        kind: DocumentKind,
        #[arg(
            long,
            value_name = "FILE",
            help = "Replace the note body with FILE, or '-' for stdin"
        )]
        set: Option<PathBuf>,
        #[arg(
            long,
            value_name = "FILE",
            conflicts_with = "set",
            help = "Append FILE (or '-' for stdin) to the note body"
        )]
        append: Option<PathBuf>,
    },
    /// Show the full speaker-labeled meeting transcript
    Transcript { id: String },
    /// Export a meeting to Markdown or JSON
    Export {
        id: String,
        #[arg(long, value_enum, default_value_t = ExportFormat::Markdown)]
        format: ExportFormat,
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
        #[arg(long, requires = "output", help = "Replace an existing output file")]
        force: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum SearchKindArg {
    Title,
    Note,
    Summary,
    Transcript,
}

impl From<SearchKindArg> for SearchKind {
    fn from(kind: SearchKindArg) -> Self {
        match kind {
            SearchKindArg::Title => Self::Title,
            SearchKindArg::Note => Self::Note,
            SearchKindArg::Summary => Self::Summary,
            SearchKindArg::Transcript => Self::Transcript,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum DocumentKind {
    #[default]
    Note,
    Summary,
    All,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum ExportFormat {
    #[default]
    Markdown,
    Json,
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use clap::CommandFactory;

    #[test]
    fn parses_meeting_list_filters() {
        let args = Args::parse_from([
            "fmtr", "--json", "meetings", "list", "--query", "planning", "--limit", "10",
        ]);

        assert!(args.json);
        let Command::Meetings { command } = args.command else {
            panic!("expected meetings command");
        };
        let MeetingCommand::List { query, limit, .. } = command else {
            panic!("expected list command");
        };
        assert_eq!(query.as_deref(), Some("planning"));
        assert_eq!(limit, 10);
    }

    #[test]
    fn help_exposes_mcp_and_export() {
        let help = Args::command().render_long_help().to_string();
        assert!(help.contains("meetings"));
        assert!(help.contains("mcp"));
        assert!(help.contains("doctor"));

        let Command::Meetings { command } = Args::parse_from([
            "fmtr",
            "meetings",
            "export",
            "meeting-1",
            "--format",
            "json",
        ])
        .command
        else {
            panic!("expected meetings command");
        };
        assert!(matches!(
            command,
            MeetingCommand::Export {
                format: ExportFormat::Json,
                ..
            }
        ));
    }

    #[test]
    fn parses_transcript_command() {
        let Command::Meetings { command } =
            Args::parse_from(["fmtr", "meetings", "transcript", "meeting-1"]).command
        else {
            panic!("expected meetings command");
        };
        assert!(matches!(
            command,
            MeetingCommand::Transcript { id } if id == "meeting-1"
        ));
    }

    #[test]
    fn parses_search_filters_and_requires_query_or_speaker() {
        let Command::Meetings { command } = Args::parse_from([
            "fmtr",
            "meetings",
            "search",
            "--speaker",
            "bob",
            "--kind",
            "transcript",
        ])
        .command
        else {
            panic!("expected meetings command");
        };
        let MeetingCommand::Search {
            query,
            speaker,
            kind,
            limit,
            offset,
        } = command
        else {
            panic!("expected search command");
        };
        assert_eq!(query, None);
        assert_eq!(speaker.as_deref(), Some("bob"));
        assert_eq!(kind, vec![SearchKindArg::Transcript]);
        assert_eq!(limit, 20);
        assert_eq!(offset, 0);

        assert!(Args::try_parse_from(["fmtr", "meetings", "search"]).is_err());
    }

    #[test]
    fn parses_new_command_with_note_source() {
        let Command::Meetings { command } = Args::parse_from([
            "fmtr", "meetings", "new", "--title", "Planning", "--note", "-",
        ])
        .command
        else {
            panic!("expected meetings command");
        };
        let MeetingCommand::New { title, note } = command else {
            panic!("expected new command");
        };
        assert_eq!(title, "Planning");
        assert_eq!(note.as_deref(), Some(Path::new("-")));

        assert!(Args::try_parse_from(["fmtr", "meetings", "new"]).is_err());
    }

    #[test]
    fn note_edit_flags_are_mutually_exclusive_and_conflict_with_kind() {
        assert!(
            Args::try_parse_from([
                "fmtr",
                "meetings",
                "note",
                "meeting-1",
                "--set",
                "a.md",
                "--append",
                "b.md",
            ])
            .is_err()
        );
        assert!(
            Args::try_parse_from([
                "fmtr",
                "meetings",
                "note",
                "meeting-1",
                "--kind",
                "summary",
                "--set",
                "a.md",
            ])
            .is_err()
        );

        let Command::Meetings { command } = Args::parse_from([
            "fmtr",
            "meetings",
            "note",
            "meeting-1",
            "--append",
            "extra.md",
        ])
        .command
        else {
            panic!("expected meetings command");
        };
        assert!(matches!(
            command,
            MeetingCommand::Note {
                set: None,
                append: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn parses_import_command_with_optional_title() {
        let Command::Import {
            file,
            title,
            transcribe,
        } = Args::parse_from(["fmtr", "import", "meeting.m4a"]).command
        else {
            panic!("expected import command");
        };
        assert_eq!(file, Path::new("meeting.m4a"));
        assert_eq!(title, None);
        assert!(!transcribe);

        let Command::Import { title, .. } =
            Args::parse_from(["fmtr", "import", "meeting.m4a", "--title", "Weekly sync"]).command
        else {
            panic!("expected import command");
        };
        assert_eq!(title.as_deref(), Some("Weekly sync"));

        assert!(Args::try_parse_from(["fmtr", "import"]).is_err());
    }

    #[test]
    fn parses_import_transcribe_flag_and_transcribe_command() {
        let Command::Import { transcribe, .. } =
            Args::parse_from(["fmtr", "import", "meeting.m4a", "--transcribe"]).command
        else {
            panic!("expected import command");
        };
        assert!(transcribe);

        let Command::Transcribe { id } =
            Args::parse_from(["fmtr", "transcribe", "meeting-1"]).command
        else {
            panic!("expected transcribe command");
        };
        assert_eq!(id, "meeting-1");

        assert!(Args::try_parse_from(["fmtr", "transcribe"]).is_err());
    }

    #[test]
    fn export_force_requires_an_output_path() {
        assert!(
            Args::try_parse_from(["fmtr", "meetings", "export", "meeting-1", "--force"]).is_err()
        );
    }

    #[test]
    fn public_docs_and_skill_cover_the_command_contract() {
        let docs = include_str!("../../../docs/reference/cli.mdx");
        let skill = concat!(
            include_str!("../../../skills/fmtr/references/cli.md"),
            include_str!("../../../skills/fmtr/references/setup.md"),
        );
        let command = Args::command();
        let mut paths = Vec::new();
        collect_leaf_commands(&command, "", &mut paths);

        for path in paths {
            assert!(docs.contains(&path), "CLI docs are missing `{path}`");
            assert!(skill.contains(&path), "fmtr skill is missing `{path}`");
        }
        assert_options_are_documented(&command, docs);
    }

    #[test]
    fn cli_contract_matches_snapshot() {
        let contract: serde_json::Value =
            serde_json::from_str(&cli_docs::generate_json(&Args::command())).unwrap();
        insta::assert_json_snapshot!("cli_contract", canonicalize_json(contract));
    }

    fn canonicalize_json(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(object) => serde_json::Value::Object(
                object
                    .into_iter()
                    .map(|(key, value)| (key, canonicalize_json(value)))
                    .collect::<std::collections::BTreeMap<_, _>>()
                    .into_iter()
                    .collect(),
            ),
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.into_iter().map(canonicalize_json).collect())
            }
            value => value,
        }
    }

    fn collect_leaf_commands(command: &clap::Command, prefix: &str, paths: &mut Vec<String>) {
        for subcommand in command
            .get_subcommands()
            .filter(|subcommand| subcommand.get_name() != "help")
        {
            let path = if prefix.is_empty() {
                subcommand.get_name().to_string()
            } else {
                format!("{prefix} {}", subcommand.get_name())
            };
            if subcommand
                .get_subcommands()
                .any(|child| child.get_name() != "help")
            {
                collect_leaf_commands(subcommand, &path, paths);
            } else {
                paths.push(path);
            }
        }
    }

    fn assert_options_are_documented(command: &clap::Command, docs: &str) {
        for argument in command.get_arguments() {
            if let Some(long) = argument.get_long() {
                assert!(
                    docs.contains(&format!("--{long}")),
                    "CLI docs are missing `--{long}`"
                );
            }
        }
        for subcommand in command.get_subcommands() {
            assert_options_are_documented(subcommand, docs);
        }
    }
}
