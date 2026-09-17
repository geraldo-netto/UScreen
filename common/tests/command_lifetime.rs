//! T328: disposable child trees, isolated subreapers, no host setup commands.
#![cfg(all(feature = "commands", target_os = "linux"))]
use std::{io, path::Path, process::Command, time::Duration};
use uscreen_config::commands::{AsyncCommandExt, SyncCommandExt};

const WORK: &str = "echo $$ > \"$1/parent\"; sh -c 'echo $$ > \"$1/worker\"; sleep 0.3; echo late > \"$1/late\"' sh \"$1\" & wait";

fn isolated(name: &str, exercise: impl FnOnce(&Path)) {
    if std::env::var("USCREEN_T328_CASE").as_deref() != Ok(name) {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env("USCREEN_T328_CASE", name)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    // Only this disposable test process adopts grandchildren for cleanup.
    unsafe {
        libc::alarm(10);
        assert_eq!(libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0), 0);
    }
    let dir = tempfile::tempdir().unwrap();
    exercise(dir.path());
}

fn assert_retired(root: &Path) {
    std::thread::sleep(Duration::from_millis(450));
    // Reap even on the pre-fix failure path; these fixture workers run <0.5s.
    unsafe { while libc::waitpid(-1, std::ptr::null_mut(), 0) > 0 {} }
    assert!(
        !root.join("late").exists(),
        "T328: descendant wrote after cancellation returned"
    );
    let worker: u32 = std::fs::read_to_string(root.join("worker"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(!Path::new(&format!("/proc/{worker}")).exists());
}

#[test]
fn t328_sync_timeout_retires_descendants() {
    isolated("t328_sync_timeout_retires_descendants", |root| {
        let error = Command::new("sh")
            .args(["-c", WORK, "sh"])
            .arg(root)
            .output_timeout(Duration::from_millis(80))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_retired(root);
    });
}

#[test]
fn t328_async_timeout_retires_descendants() {
    isolated("t328_async_timeout_retires_descendants", |root| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let error = tokio::process::Command::new("sh")
                .args(["-c", WORK, "sh"])
                .arg(root)
                .output_timeout(Duration::from_millis(80))
                .await
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        });
        assert_retired(root);
    });
}

#[test]
fn t328_async_cancellation_retires_descendants() {
    isolated("t328_async_cancellation_retires_descendants", |root| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let path = root.to_owned();
            let task = tokio::spawn(async move {
                tokio::process::Command::new("sh")
                    .args(["-c", WORK, "sh"])
                    .arg(path)
                    .output_timeout(Duration::from_secs(2))
                    .await
            });
            tokio::time::timeout(Duration::from_secs(1), async {
                while !root.join("worker").exists() {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            task.abort();
            let _ = task.await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        });
        assert_retired(root);
    });
}

#[test]
fn t328_success_preserves_status_and_output() {
    let out = Command::new("sh")
        .args(["-c", "printf out; printf err >&2; exit 7"])
        .output_bounded()
        .unwrap();
    assert_eq!(out.status.code(), Some(7));
    assert_eq!(out.stdout, b"out");
    assert_eq!(out.stderr, b"err");
}

#[test]
fn t328_successful_async_command_keeps_completed_status() {
    isolated(
        "t328_successful_async_command_keeps_completed_status",
        |root| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let output = runtime.block_on(async {
            tokio::process::Command::new("sh")
                .args(["-c", "(sleep 0.1; printf done > \"$1/done\") & printf out; printf err >&2; exit 7", "sh"])
                .arg(root).output_bounded().await.unwrap()
        });
            assert_eq!(output.status.code(), Some(7));
            assert_eq!(output.stdout, b"out");
            assert_eq!(output.stderr, b"err");
            unsafe { while libc::waitpid(-1, std::ptr::null_mut(), 0) > 0 {} }
            assert!(
                root.join("done").exists(),
                "T328: success must not run cancellation cleanup"
            );
        },
    );
}

#[test]
fn t328_detached_work_reports_cancellation_limit() {
    isolated("t328_detached_work_reports_cancellation_limit", |root| {
        let error = Command::new("sh")
            .args([
                "-c",
                "setsid sh -c 'sleep 0.3; printf done > \"$1/done\"' sh \"$1\" & wait",
                "sh",
            ])
            .arg(root)
            .output_timeout(Duration::from_millis(80))
            .unwrap_err();
        unsafe { while libc::waitpid(-1, std::ptr::null_mut(), 0) > 0 {} }
        assert!(root.join("done").exists());
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(error
            .to_string()
            .contains("detached or privileged work may still be running"));
    });
}
