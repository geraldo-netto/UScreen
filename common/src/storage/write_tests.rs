//! T566: successful-write diagnostics must follow the filesystem commit.
use super::*;

fn recorded_write(config: &FileConfig, path: &Path) -> (Result<()>, String) {
    let log = tempfile::NamedTempFile::new().unwrap();
    let writer = log.reopen().unwrap();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(move || writer.try_clone().unwrap())
        .finish();
    let result = tracing::subscriber::with_default(subscriber, || config.write_at(path));
    (result, std::fs::read_to_string(log.path()).unwrap())
}

#[cfg(target_os = "linux")]
#[test]
fn t566_failed_persistence_never_reports_a_successful_change() {
    use std::os::fd::AsRawFd;
    let original = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(original.path(), "fps = 60\n").unwrap();
    // procfs permits reading the old regular file but forbids creating the
    // temporary replacement, including under root in the container suite.
    let path = PathBuf::from(format!("/proc/self/fd/{}", original.as_file().as_raw_fd()));
    let changed = FileConfig {
        fps: 30,
        ..Default::default()
    };
    let (result, log) = recorded_write(&changed, &path);
    assert!(result.is_err(), "T566: fixture must fail at persistence");
    assert_eq!(
        std::fs::read_to_string(original.path()).unwrap(),
        "fps = 60\n"
    );
    assert!(
        !log.contains("Config written:"),
        "T566: failed commit claimed success: {log}"
    );
}

#[test]
fn t566_success_reports_only_committed_changes_and_noop_stays_quiet() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    let original = FileConfig::default();
    original.write_at(&path).unwrap();
    let changed = FileConfig {
        fps: 30,
        ..original
    };
    let (result, log) = recorded_write(&changed, &path);
    result.unwrap();
    assert_eq!(FileConfig::load_at(&path).fps, 30);
    assert!(
        log.contains("Config written: fps = 30 (was 60)"),
        "T566: {log}"
    );
    let (result, log) = recorded_write(&changed, &path);
    result.unwrap();
    assert!(
        !log.contains("Config written:"),
        "T566: unchanged configuration: {log}"
    );
}
