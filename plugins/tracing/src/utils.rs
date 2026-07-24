use std::path::PathBuf;
use std::{fs, io};

use file_rotate::{ContentLimit, FileRotate, compression::Compression, suffix::AppendCount};
use tracing_appender::non_blocking::WorkerGuard;

pub fn cleanup_old_daily_logs(logs_dir: &PathBuf) -> io::Result<()> {
    if !logs_dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(logs_dir)? {
        let entry = entry?;
        let path = entry.path();

        if let Some(filename) = path.file_name().and_then(|n| n.to_str())
            && filename.starts_with("log.")
            && filename.len() > 4
        {
            let suffix = &filename[4..];
            if suffix.chars().all(|c| c.is_ascii_digit() || c == '-') {
                let _ = fs::remove_file(path);
            }
        }
    }

    Ok(())
}

pub(crate) fn make_file_writer_if_enabled(
    enabled: bool,
    logs_dir: &PathBuf,
) -> Option<(tracing_appender::non_blocking::NonBlocking, WorkerGuard)> {
    if !enabled {
        return None;
    }

    let _ = cleanup_old_daily_logs(logs_dir);

    let log_path = logs_dir.join("app.log");
    let file_appender = FileRotate::new(
        log_path,
        AppendCount::new(5),
        ContentLimit::Bytes(5 * 1024 * 1024),
        Compression::None,
        None,
    );

    let redacting_appender = crate::redaction::RedactingWriter::new(file_appender);

    let (non_blocking, guard) = tracing_appender::non_blocking(redacting_appender);
    Some((non_blocking, guard))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn cleanup_old_daily_logs_removes_matching_files() {
        let temp = tempdir().unwrap();
        let logs_dir = temp.path().to_path_buf();

        fs::write(logs_dir.join("log.2024-01-15"), "old log").unwrap();
        fs::write(logs_dir.join("log.2024-01-16"), "old log").unwrap();
        fs::write(logs_dir.join("log.2024-12-31"), "old log").unwrap();

        cleanup_old_daily_logs(&logs_dir).unwrap();

        assert!(!logs_dir.join("log.2024-01-15").exists());
        assert!(!logs_dir.join("log.2024-01-16").exists());
        assert!(!logs_dir.join("log.2024-12-31").exists());
    }

    #[test]
    fn cleanup_old_daily_logs_preserves_non_matching() {
        let temp = tempdir().unwrap();
        let logs_dir = temp.path().to_path_buf();

        fs::write(logs_dir.join("app.log"), "current log").unwrap();
        fs::write(logs_dir.join("app.log.1"), "rotated log").unwrap();
        fs::write(logs_dir.join("other.txt"), "other file").unwrap();
        fs::write(logs_dir.join("log.2024-01-15"), "old log").unwrap();

        cleanup_old_daily_logs(&logs_dir).unwrap();

        assert!(logs_dir.join("app.log").exists());
        assert!(logs_dir.join("app.log.1").exists());
        assert!(logs_dir.join("other.txt").exists());
        assert!(!logs_dir.join("log.2024-01-15").exists());
    }

    #[test]
    fn cleanup_old_daily_logs_handles_empty_dir() {
        let temp = tempdir().unwrap();
        let logs_dir = temp.path().to_path_buf();

        let result = cleanup_old_daily_logs(&logs_dir);
        assert!(result.is_ok());
    }

    #[test]
    fn cleanup_old_daily_logs_handles_nonexistent_dir() {
        let logs_dir = PathBuf::from("/nonexistent/path/that/does/not/exist");

        let result = cleanup_old_daily_logs(&logs_dir);
        assert!(result.is_ok());
    }

    #[test]
    fn cleanup_old_daily_logs_preserves_log_without_date_suffix() {
        let temp = tempdir().unwrap();
        let logs_dir = temp.path().to_path_buf();

        fs::write(logs_dir.join("log.txt"), "log file").unwrap();
        fs::write(logs_dir.join("log.backup"), "backup").unwrap();

        cleanup_old_daily_logs(&logs_dir).unwrap();

        assert!(logs_dir.join("log.txt").exists());
        assert!(logs_dir.join("log.backup").exists());
    }
}
