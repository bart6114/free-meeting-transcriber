use std::path::Path;

const AGENTS_CONTENT: &str = include_str!("agents-content.md");

pub fn write_agents_file(base_dir: &Path) -> std::io::Result<()> {
    let agents_path = base_dir.join("AGENTS.md");
    std::fs::write(agents_path, AGENTS_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_guidance_prefers_typed_meeting_interfaces() {
        for tool in ["list_meetings", "get_meeting", "get_meeting_transcript"] {
            assert!(AGENTS_CONTENT.contains(tool));
        }

        assert!(AGENTS_CONTENT.contains("fmtr --json meetings list"));
        assert!(!AGENTS_CONTENT.contains("--base ."));
        assert!(AGENTS_CONTENT.contains("--vault-path ABSOLUTE_VAULT_DIR"));
        assert!(AGENTS_CONTENT.contains("Do not use `find`,"));
        assert!(AGENTS_CONTENT.contains("direct SQLite queries"));
    }
}
