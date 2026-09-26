//! Capture the supported pre-subscriber log facade only in isolated test processes.
use std::sync::Mutex;
use tracing::log::{LevelFilter, Log, Metadata, Record};

static LOG: Captured = Captured(Mutex::new(String::new()));
struct Captured(Mutex<String>);

impl Log for Captured {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &Record<'_>) {
        use std::fmt::Write;
        writeln!(self.0.lock().unwrap(), "{}", record.args()).unwrap();
    }
    fn flush(&self) {}
}

pub fn enable() {
    tracing::log::set_logger(&LOG).unwrap();
    tracing::log::set_max_level(LevelFilter::Trace);
}

pub fn text() -> String {
    LOG.0.lock().unwrap().clone()
}

/// T590: global log-facade state must never leak into other parallel tests.
pub fn isolated(name: &str) -> bool {
    if std::env::var_os("BLENT_T590_LOG_CHILD").is_some() {
        enable();
        return false;
    }
    use blent_config::commands::SyncCommandExt;
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("BLENT_T590_LOG_CHILD", "1")
        .output_timeout(std::time::Duration::from_secs(15))
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}
