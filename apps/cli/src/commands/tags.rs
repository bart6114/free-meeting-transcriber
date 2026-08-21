use std::path::Path;

use crate::cli::TagsCommand;
use crate::{Error, Result, output};
use hypr_vault_write::SessionStore;

pub async fn run(vault: &Path, command: TagsCommand, json: bool) -> Result<()> {
    match command {
        TagsCommand::List => {
            let tags = SessionStore::new(vault.to_path_buf())
                .list_tags()
                .await
                .map_err(|error| Error::operation("list tags", error.to_string()))?;
            let rendered = if json {
                output::json("tags.list", &tags, None)?
            } else if tags.is_empty() {
                "No tags found.".to_string()
            } else {
                tags.iter()
                    .map(|tag| tag.name.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            output::emit(&rendered);
            Ok(())
        }
    }
}
