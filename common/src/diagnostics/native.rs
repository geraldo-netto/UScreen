//! Discovery and bounded command execution reuse the platform service adapters.
use super::{collect_with, Probe, Report};
use crate::{commands::SyncCommandExt, platform};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

struct Native {
    search_path: OsString,
}
impl Probe for Native {
    fn find(&mut self, name: &str) -> Option<PathBuf> {
        platform::programs::find_in(name, &self.search_path)
    }
    fn run(&mut self, path: &Path, argument: &str) -> Result<Vec<u8>, String> {
        execute(Command::new(path).arg(argument), Duration::from_secs(2))
    }
}

fn execute(command: &mut Command, timeout: Duration) -> Result<Vec<u8>, String> {
    let output = command
        .output_timeout(timeout)
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!("version command exited with {}", output.status));
    }
    Ok(output.stdout)
}

pub fn collect() -> Report {
    let mut native = Native {
        search_path: std::env::var_os("PATH").unwrap_or_default(),
    };
    collect_with(&mut native, platform::capabilities())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t533_command_fixture() {
        if std::env::var_os("BLENT_T533_WAIT").is_some() {
            std::thread::sleep(Duration::from_secs(5));
        }
    }

    #[test]
    fn t533_native_discovery_and_failure_keep_process_contracts() {
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join(platform::executable_name("fixture café"));
        std::fs::copy(std::env::current_exe().unwrap(), &binary).unwrap();
        let mut native = Native {
            search_path: std::env::join_paths([root.path()]).unwrap(),
        };
        assert_eq!(native.find("fixture café"), Some(binary.clone()));
        assert!(native.find("missing").is_none());
        assert!(!native.run(&binary, "--help").unwrap().is_empty());
        assert!(native
            .run(&binary, "--invalid-t533-argument")
            .unwrap_err()
            .contains("exited"));
        assert!(native.run(&root.path().join("missing"), "version").is_err());
        let result = execute(
            Command::new(&binary)
                .args([
                    "--exact",
                    "diagnostics::native::tests::t533_command_fixture",
                    "--nocapture",
                ])
                .env("BLENT_T533_WAIT", "1"),
            Duration::from_millis(100),
        );
        assert!(result.unwrap_err().contains("timed out"));
        // Version probes must never start ADB discovery or a display backend.
        let report = collect();
        assert_eq!(report.tools.len(), 2);
        assert!(report
            .lines()
            .iter()
            .any(|line| line.contains("Tablet connection: unverified")));
    }
}
