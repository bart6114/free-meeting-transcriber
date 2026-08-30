use std::path::Path;

pub fn migrate_legacy_gguf_files(global_base: &Path) {
    let models_dir = global_base.join("models/llm");
    let _ = std::fs::create_dir_all(&models_dir);

    if let Ok(entries) = std::fs::read_dir(global_base) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) == Some("gguf")
                && let Some(name) = path.file_name()
            {
                let _ = std::fs::rename(&path, models_dir.join(name));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_only_root_gguf_files_into_the_llm_models_directory() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("legacy.gguf"), b"model").unwrap();
        std::fs::write(root.join("keep.txt"), b"user file").unwrap();
        std::fs::create_dir(root.join("nested")).unwrap();
        std::fs::write(root.join("nested/model.gguf"), b"nested model").unwrap();

        migrate_legacy_gguf_files(root);

        assert_eq!(
            std::fs::read(root.join("models/llm/legacy.gguf")).unwrap(),
            b"model"
        );
        assert!(!root.join("legacy.gguf").exists());
        assert_eq!(std::fs::read(root.join("keep.txt")).unwrap(), b"user file");
        assert_eq!(
            std::fs::read(root.join("nested/model.gguf")).unwrap(),
            b"nested model"
        );
    }

    #[test]
    fn remains_best_effort_when_the_models_path_cannot_be_created() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("models"), b"not a directory").unwrap();
        std::fs::write(root.join("legacy.gguf"), b"model").unwrap();

        migrate_legacy_gguf_files(root);

        assert_eq!(std::fs::read(root.join("legacy.gguf")).unwrap(), b"model");
    }
}
