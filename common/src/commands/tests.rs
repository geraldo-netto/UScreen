use super::*;

// T496: re-exec the native test executable; no shell or external sleep program.
fn fixture(path: &std::path::Path, role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "commands::tests::t496_fixture_process",
            "--nocapture",
        ])
        .env("BLENT_T496_PATH", path)
        .env("BLENT_T496_ROLE", role);
    command
}

#[test]
fn t496_fixture_process() {
    let Some(path) = std::env::var_os("BLENT_T496_PATH") else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    let stage = path.with_extension("tmp");
    std::fs::write(&stage, std::process::id().to_string()).unwrap();
    std::fs::rename(stage, &path).unwrap();
    let role = std::env::var("BLENT_T496_ROLE").unwrap();
    match role.as_str() {
        "input" => {
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes).unwrap();
            std::fs::write(path.with_extension("input"), bytes).unwrap();
        }
        "denied" => std::thread::sleep(Duration::from_millis(400)),
        "wait" => std::thread::sleep(Duration::from_secs(10)),
        _ => panic!("T496: unknown fixture role"),
    }
    std::fs::write(path.with_extension("done"), "done").unwrap();
}

fn assert_exited(pid: u32) {
    #[cfg(target_os = "linux")]
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    #[cfg(windows)]
    {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::{Foundation::WAIT_OBJECT_0, System::Threading::*};
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        if !handle.is_null() {
            let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
            assert_eq!(
                unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) },
                WAIT_OBJECT_0
            );
        }
    }
}

fn fixture_timeout() -> Duration {
    // Keep the original Linux deadline; allow native executable startup on Windows.
    if cfg!(windows) {
        Duration::from_millis(500)
    } else {
        Duration::from_millis(50)
    }
}

#[test]
fn t328_denied_signal_does_not_extend_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pid");
    let child = fixture(&path, "denied").spawn().unwrap();
    let pid = child.id();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !path.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "T496: fixture did not start"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let start = std::time::Instant::now();
    // Isolate the EPERM result of a privileged child without authorizing
    // real system setup or depending on root in the normal test suite.
    reap_after_signal(child, Err(io::Error::from(io::ErrorKind::PermissionDenied)));
    let elapsed = start.elapsed();
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        path.with_extension("done").exists(),
        "T328: denied work can continue after the deadline"
    );
    assert_exited(pid);
    assert!(
        elapsed < Duration::from_millis(200),
        "T328: denied cancellation blocked for {elapsed:?}"
    );
    assert!(timed_out().to_string().contains("may still be running"));
}

#[test]
fn t268_lifecycle_deadlines_cover_cli_and_shipped_service_budgets() {
    assert!(daemon_command_timeout(false) > DAEMON_STOP_TIMEOUT);
    assert!(SERVICE_STOP_TIMEOUT > DAEMON_STOP_TIMEOUT);
    assert!(daemon_command_timeout(true) > SERVICE_STOP_TIMEOUT * 2);
    for unit in [
        include_str!("../../../scripts/blent.service"),
        include_str!("../../../packaging/blent.service"),
    ] {
        let seconds: u64 = unit
            .lines()
            .find_map(|line| line.strip_prefix("TimeoutStopSec="))
            .expect("T268: explicit service stop deadline")
            .parse()
            .unwrap();
        assert_eq!(Duration::from_secs(seconds), SERVICE_STOP_TIMEOUT);
    }
}

fn assert_reaped(path: &std::path::Path) {
    let pid = std::fs::read_to_string(path).unwrap();
    assert_exited(pid.trim().parse().unwrap());
}

// T094: subprocess deadlines must kill and reap, not abandon hung commands.
#[test]
fn t094_sync_command_is_bounded_and_reaped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pid");
    let start = std::time::Instant::now();
    let result = fixture(&path, "wait").output_timeout(fixture_timeout());
    assert!(result.is_err_and(|e| e.kind() == io::ErrorKind::TimedOut));
    assert!(start.elapsed() < Duration::from_millis(800));
    assert_reaped(&path);
}

#[tokio::test]
async fn t094_async_command_is_bounded_and_reaped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pid");
    let result = tokio::process::Command::from(fixture(&path, "wait"))
        .output_timeout(fixture_timeout())
        .await;
    assert!(result.is_err_and(|e| e.kind() == io::ErrorKind::TimedOut));
    assert_reaped(&path);
}

#[tokio::test]
async fn t094_cancelled_command_is_reaped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pid");
    let task_path = path.clone();
    let task = tokio::spawn(async move {
        tokio::process::Command::from(fixture(&task_path, "wait"))
            .output_bounded()
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !path.exists() {
            assert!(
                !task.is_finished(),
                "T496: fixture failed before publishing its PID"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("T496: fixture readiness deadline");
    task.abort();
    let _ = task.await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_reaped(&path);
}

#[cfg(target_os = "linux")]
fn deny_kill_in_test_thread() {
    // T590: emulate EPERM without signalling any real process or changing UID.
    // The filter applies only to this isolated child's test thread.
    let mut instructions = [
        libc::sock_filter {
            code: (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16,
            jt: 0,
            jf: 0,
            k: 0,
        },
        libc::sock_filter {
            code: (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16,
            jt: 0,
            jf: 1,
            k: libc::SYS_kill as u32,
        },
        libc::sock_filter {
            code: (libc::BPF_RET | libc::BPF_K) as u16,
            jt: 0,
            jf: 0,
            k: libc::SECCOMP_RET_ERRNO | libc::EPERM as u32,
        },
        libc::sock_filter {
            code: (libc::BPF_RET | libc::BPF_K) as u16,
            jt: 0,
            jf: 0,
            k: libc::SECCOMP_RET_ALLOW,
        },
    ];
    let filter = libc::sock_fprog {
        len: instructions.len() as u16,
        filter: instructions.as_mut_ptr(),
    };
    unsafe {
        assert_eq!(libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0), 0);
        assert_eq!(
            libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &filter),
            0
        );
        assert_eq!(libc::kill(std::process::id() as libc::pid_t, 0), -1);
    }
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EPERM));
}

#[cfg(target_os = "linux")]
#[test]
fn t590_denied_group_signal_reports_unretired_work() {
    const NAME: &str = "commands::tests::t590_denied_group_signal_reports_unretired_work";
    if std::env::var_os("BLENT_T590_DENIED_SIGNAL").is_none() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", NAME, "--nocapture"])
            .env("BLENT_T590_DENIED_SIGNAL", "1")
            .output_timeout(Duration::from_secs(10))
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let log = tempfile::NamedTempFile::new().unwrap();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(log.reopen().unwrap())
        .finish();
    deny_kill_in_test_thread();
    tracing::subscriber::with_default(subscriber, || terminate_group(std::process::id()));
    assert!(std::fs::read_to_string(log.path())
        .unwrap()
        .contains("Command group could not be terminated; delegated work may continue"));
}

#[tokio::test]
async fn t609_stdin_delivery_closes_input_and_preserves_bytes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("pid");
    let input = b"secret ' \" \n\0 end";
    let output = tokio::process::Command::from(fixture(&path, "input"))
        .output_input_timeout(Some(input), Duration::from_secs(2))
        .await
        .unwrap();
    assert!(output.status.success());
    assert_eq!(std::fs::read(path.with_extension("input")).unwrap(), input);
    assert_reaped(&path);
}

#[tokio::test]
async fn t609_timeout_covers_blocked_stdin_write() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("pid");
    let input = vec![0; 1024 * 1024];
    let error = tokio::process::Command::from(fixture(&path, "wait"))
        .output_input_timeout(Some(&input), fixture_timeout())
        .await
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_reaped(&path);
}

#[tokio::test]
async fn t609_cancelled_stdin_writer_is_reaped() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("pid");
    let task_path = path.clone();
    let task = tokio::spawn(async move {
        tokio::process::Command::from(fixture(&task_path, "wait"))
            .output_input_timeout(Some(&vec![0; 1024 * 1024]), Duration::from_secs(10))
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    let _ = task.await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_reaped(&path);
}
