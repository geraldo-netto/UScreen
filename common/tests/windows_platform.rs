//! T493: Windows paths and owned subprocess trees, without a tablet or shell.
#![cfg(all(windows, feature = "storage", feature = "commands"))]
use blent_config::commands::{AsyncCommandExt, SyncCommandExt};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

fn fixture(role: &str, directory: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "t493_child", "--nocapture"])
        .env("BLENT_T493_ROLE", role)
        .env("BLENT_T493_DIR", directory);
    command
}

#[test]
fn t493_child() {
    let Ok(role) = std::env::var("BLENT_T493_ROLE") else {
        return;
    };
    let root = std::path::PathBuf::from(std::env::var_os("BLENT_T493_DIR").unwrap());
    match role.as_str() {
        "path" => {
            let path = blent_config::config_path().unwrap();
            assert!(
                path.is_absolute(),
                "T493: relative Windows configuration path: {path:?}"
            );
            assert!(path.ends_with("blent/config.toml"));
        }
        "parent" => {
            let mut child = fixture("worker", &root).spawn().unwrap();
            std::fs::write(root.join("parent-ready"), "ready").unwrap();
            child.wait().unwrap();
        }
        "worker" => {
            std::fs::write(root.join("worker-ready"), "ready").unwrap();
            std::thread::sleep(Duration::from_secs(3));
            std::fs::write(root.join("escaped"), "not retired").unwrap();
        }
        _ => panic!("unknown test role"),
    }
}

#[test]
fn t493_windows_config_does_not_depend_on_unix_environment() {
    let root = tempfile::tempdir().unwrap();
    let output = fixture("path", root.path())
        .env_remove("HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("APPDATA")
        .env_remove("LOCALAPPDATA")
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "T493: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_retired(root: &Path) {
    assert!(
        root.join("worker-ready").exists(),
        "T493: fixture never spawned its descendant"
    );
    std::thread::sleep(Duration::from_secs(3));
    assert!(
        !root.join("escaped").exists(),
        "T493: descendant survived command retirement"
    );
}

#[test]
fn t493_sync_timeout_retires_descendants() {
    let root = tempfile::tempdir().unwrap();
    let result = fixture("parent", root.path()).output_timeout(Duration::from_secs(1));
    assert!(result.is_err_and(|error| error.kind() == std::io::ErrorKind::TimedOut));
    assert_retired(root.path());
}

#[tokio::test]
async fn t493_async_timeout_retires_descendants() {
    let root = tempfile::tempdir().unwrap();
    let result = tokio::process::Command::from(fixture("parent", root.path()))
        .output_timeout(Duration::from_secs(1))
        .await;
    assert!(result.is_err_and(|error| error.kind() == std::io::ErrorKind::TimedOut));
    assert_retired(root.path());
}

#[tokio::test]
async fn t493_async_cancel_retires_descendants() {
    let root = tempfile::tempdir().unwrap();
    let command = fixture("parent", root.path());
    let task = tokio::spawn(async move {
        tokio::process::Command::from(command)
            .output_bounded()
            .await
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while !root.path().join("worker-ready").exists() {
        assert!(Instant::now() < deadline, "T493: worker failed to start");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    task.abort();
    let _ = task.await;
    assert_retired(root.path());
}

#[test]
fn t493_windows_store_round_trips_unicode_and_spaces() {
    let root = tempfile::tempdir().unwrap();
    let store =
        blent_config::storage::ConfigStore::new(root.path().join("space café 東京/config.toml"));
    let saved = store
        .update(|config| {
            config.fps = 30;
            Ok(())
        })
        .unwrap();
    assert_eq!(store.load(), saved);
}

#[test]
fn t493_runtime_directory_is_private_and_pinned() {
    use blent_config::windows::private::Directory;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("private café 東京");
    let directory = Directory::create(&path).unwrap();
    assert_eq!(directory.path(), path);
    assert!(std::fs::rename(&path, root.path().join("moved")).is_err());
    assert!(Directory::create(&path).is_ok());
    drop(directory);
    std::fs::rename(&path, root.path().join("moved")).unwrap();
}

#[test]
fn t493_runtime_rejects_relative_files_and_inherited_permissions() {
    use blent_config::windows::private::Directory;
    let root = tempfile::tempdir().unwrap();
    assert!(Directory::create(Path::new("relative")).is_err());
    let file = root.path().join("file");
    std::fs::write(&file, "not a directory").unwrap();
    assert!(Directory::create(&file).is_err());
    let inherited = root.path().join("inherited");
    std::fs::create_dir(&inherited).unwrap();
    assert!(Directory::create(&inherited).is_err());
    assert!(Directory::create(&root.path().join("missing/child")).is_err());
}

#[test]
fn t493_owned_process_checks_identity_before_retirement() {
    use blent_config::windows::process::Identity;
    assert!(Identity::read(0).is_err());
    let current = Identity::read(std::process::id()).unwrap();
    assert!(current.is_current());
    assert!(current.retire(Duration::from_secs(1)).is_err());
    let root = tempfile::tempdir().unwrap();
    let mut child = fixture("worker", root.path()).spawn().unwrap();
    let original = Identity::read(child.id()).unwrap();
    let mut stale = original.clone();
    stale.started = stale.started.wrapping_add(1);
    assert!(!stale.is_current());
    assert!(stale.retire(Duration::from_secs(1)).is_err());
    assert!(child.try_wait().unwrap().is_none());
    original.retire(Duration::from_secs(1)).unwrap();
    child.wait().unwrap();
    assert!(!original.is_current());
}

#[test]
fn t493_runtime_lease_rejects_second_owner_and_corrupt_records() {
    use blent_config::windows::{
        private::Directory,
        runtime::{self, Lease},
    };
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("runtime");
    let directory = Directory::create(&path).unwrap();
    assert_eq!(runtime::owner_at(&directory), None);
    let lease = Lease::acquire(Directory::create(&path).unwrap()).unwrap();
    assert_eq!(lease.directory(), path);
    assert_eq!(
        runtime::owner_at(&directory).as_ref(),
        Some(lease.identity())
    );
    assert!(Lease::acquire(Directory::create(&path).unwrap()).is_err());
    let valid = std::fs::read(path.join("daemon.json")).unwrap();
    for invalid in [b"{".to_vec(), b"null".to_vec(), vec![b' '; 65537]] {
        std::fs::write(path.join("daemon.json"), invalid).unwrap();
        assert_eq!(runtime::owner_at(&directory), None);
    }
    std::fs::write(path.join("daemon.json"), valid).unwrap();
    drop(lease);
    assert_eq!(runtime::owner_at(&directory), None);
    assert!(!path.join("daemon.json").exists());
    assert!(Lease::acquire(directory).is_ok());
}
