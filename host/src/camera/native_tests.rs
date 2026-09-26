//! T572: native open/lock/lifecycle coverage in a private mount namespace.
use super::*;
use std::{ffi::CStr, os::fd::FromRawFd, os::unix::fs::MetadataExt, process::Command};

const TEST: &str = "camera::native_tests::t572_isolated_native_devices_and_session_cleanup";

#[test]
fn t572_isolated_native_devices_and_session_cleanup() {
    if std::env::var_os("BLENT_T572_MOUNTS").is_some() {
        fixture();
        return;
    }
    let mut command = Command::new("unshare");
    if unsafe { libc::geteuid() } != 0 {
        command.args(["--user", "--map-root-user"]);
    }
    let result = command
        .args(["--mount", "--fork", "--propagation", "private"])
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture"])
        .env("BLENT_T572_MOUNTS", "1")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "T572 requires private mount fixtures: {}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

fn device(root: &std::path::Path, label: &str) -> (PathBuf, std::fs::File) {
    // Fresh PTYs are character devices owned only by this fixture. No root
    // mknod capability or live webcam node is needed.
    let fd = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
    assert!(fd >= 0, "{}", std::io::Error::last_os_error());
    let master = unsafe { std::fs::File::from_raw_fd(fd) };
    assert_eq!(unsafe { libc::grantpt(fd) }, 0);
    assert_eq!(unsafe { libc::unlockpt(fd) }, 0);
    let mut name = [0; 1024];
    assert_eq!(
        unsafe { libc::ptsname_r(fd, name.as_mut_ptr(), name.len()) },
        0
    );
    let path = PathBuf::from(unsafe { CStr::from_ptr(name.as_ptr()) }.to_str().unwrap());
    let metadata = std::fs::metadata(&path).unwrap();
    let sys = root.join(format!(
        "sys/{}:{}",
        libc::major(metadata.rdev()),
        libc::minor(metadata.rdev())
    ));
    std::fs::create_dir_all(&sys).unwrap();
    std::fs::write(sys.join("name"), label).unwrap();
    (path, master)
}

fn fixture() {
    let root = tempfile::tempdir().unwrap();
    let (front, _front_owner) = device(root.path(), "Blent Front\n");
    let (rear, _rear_owner) = device(root.path(), "Blent Rear\n");
    assert!(Command::new("mount")
        .args(["--bind"])
        .arg(root.path().join("sys"))
        .arg("/sys/dev/char")
        .status()
        .unwrap()
        .success());
    let file = outputs::open_device(&front, "Blent Front").unwrap();
    assert!(outputs::open_device(&front, "Blent Front")
        .unwrap_err()
        .to_string()
        .contains("already owned"));
    assert!(outputs::open_device(&rear, "Blent Front")
        .unwrap_err()
        .to_string()
        .contains("labelled"));
    let link = root.path().join("symlink");
    std::os::unix::fs::symlink(&front, &link).unwrap();
    assert!(outputs::open_device(&link, "Blent Front").is_err());
    drop(file);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(lifecycle(root.path(), front, rear));
}

async fn lifecycle(root: &std::path::Path, front: PathBuf, rear: PathBuf) {
    tests::script(root, "ffmpeg", "import sys\nsys.exit(1)");
    tests::script(
        root,
        "adb",
        &format!(
            r#"import sys, pathlib
root=pathlib.Path({root:?})
a=sys.argv
if 'tcp:0' in a:
    (root/'mapping').write_text(a[-1]); print('34567')
elif '--list' in a: print('Usb tcp:34567 '+(root/'mapping').read_text())
elif '--remove' in a: (root/'mapping').unlink()
elif 'shell' in a and 'broadcast' in sys.stdin.read(): print('Broadcast completed: result=1')
else: print('fixture-tablet')"#
        ),
    );
    // This process is a single-test child; no sibling tests see its PATH.
    std::env::set_var("PATH", root);
    let mut options = tests::options();
    options.front_device = front;
    options.rear_device = rear;
    for stopped in [true, false] {
        let (_stop, stop) = watch::channel(stopped);
        let status = Report {
            state: watch::channel(State::Starting).0,
            preview: watch::channel(None).0,
        };
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            run_native(options.clone(), stop, status),
        )
        .await
        .expect("T572 bounded native session");
        assert_eq!(result.is_ok(), stopped, "{result:?}");
        assert!(
            !root.join("mapping").exists(),
            "T572 reverse mapping leaked"
        );
        // Both leases must retire after graceful stop and producer failure.
        drop(outputs::open_device(&options.front_device, "Blent Front").unwrap());
        drop(outputs::open_device(&options.rear_device, "Blent Rear").unwrap());
    }
}
