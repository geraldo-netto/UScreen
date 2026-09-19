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
