//! T696 permanent shared polling/native child lifecycle fixtures. No network.
use super::*;
use std::path::{Path, PathBuf};
fn fixture(root: &Path) -> PathBuf {
    let source = root.join("curl_fixture.rs");
    std::fs::write(&source, r#"
use std::io::Write;
fn main() {
    let root = std::env::current_exe().unwrap().parent().unwrap().to_owned();
    std::fs::write(root.join("started"), std::process::id().to_string()).unwrap();
    std::fs::write(root.join("arguments"), std::env::args().skip(1).collect::<Vec<_>>().join("\n")).unwrap();
    let mode = std::fs::read_to_string(root.join("mode")).unwrap_or_default();
    if mode == "fail" { std::process::exit(42); }
    if mode == "hang" { std::thread::sleep(std::time::Duration::from_secs(60)); }
    std::io::stdout().write_all(&std::fs::read(root.join("reply")).unwrap_or_default()).unwrap();
}
"#).unwrap();
    let exe = root.join(format!(
        "curl café & spaces{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(std::process::Command::new("rustc")
        .args(["--crate-name", "curl_fixture"])
        .arg(&source)
        .arg("-o")
        .arg(&exe)
        .status()
        .unwrap()
        .success());
    exe
}
async fn advance(duration: Duration) {
    tokio::time::pause();
    // Let spawned tasks register timers before moving the paused clock. A
    // yield alone does not guarantee another task was polled. Cross one timer
    // tick without changing production delays or waiting real wall time.
    tokio::time::sleep(Duration::from_millis(1)).await;
    tokio::time::advance(duration).await;
    tokio::time::resume();
}
async fn wait(message: &str, mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|error| panic!("T696 {message}: {error}"));
}
async fn receive(receiver: &mut watch::Receiver<Available>) -> Available {
    tokio::time::timeout(Duration::from_secs(5), receiver.changed())
        .await
        .unwrap()
        .unwrap();
    receiver.borrow_and_update().clone()
}
#[tokio::test]
async fn t696_polling_is_optional_async_and_preserves_last_valid_release() {
    let root = tempfile::tempdir().unwrap();
    let exe = fixture(root.path());
    let disabled = Subscription::start(false, exe.clone().into());
    assert!(disabled.task.is_none());
    disabled.shutdown().await;
    assert!(!root.path().join("started").exists());
    std::fs::write(root.path().join("reply"), r#"{"tag_name":"v999.0.0"}"#).unwrap();
    let mut subscription = Subscription::start(true, exe.into());
    assert!(!root.path().join("started").exists());
    advance(FIRST_CHECK).await;
    assert_eq!(
        receive(&mut subscription.receiver).await.as_deref(),
        Some("999.0.0")
    );
    let args = std::fs::read_to_string(root.path().join("arguments")).unwrap();
    assert!(args.ends_with(RELEASES_API));
    std::fs::write(root.path().join("mode"), "fail").unwrap();
    advance(INTERVAL).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(200), subscription.receiver.changed())
            .await
            .is_err()
    );
    assert_eq!(subscription.receiver.borrow().as_deref(), Some("999.0.0"));
    std::fs::write(root.path().join("mode"), "").unwrap();
    std::fs::write(root.path().join("reply"), r#"{"tag_name":"invalid"}"#).unwrap();
    advance(INTERVAL).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(200), subscription.receiver.changed())
            .await
            .is_err(),
        "T696 invalid tag erased valid notification"
    );
    std::fs::write(
        root.path().join("reply"),
        format!("{{\"tag_name\":\"{}\"}}", current_version()),
    )
    .unwrap();
    advance(INTERVAL).await;
    assert_eq!(receive(&mut subscription.receiver).await, None);
    subscription.shutdown().await;
}
#[tokio::test]
async fn t696_invalid_responses_byte_bounds_and_deadlines() {
    let root = tempfile::tempdir().unwrap();
    let exe = fixture(root.path());
    for bytes in [
        b"".to_vec(),
        b"not JSON".to_vec(),
        vec![255; 20],
        b"{\"tag_name\":null}".to_vec(),
        format!(
            "{{\"tag_name\":\"v999.0.0\",\"body\":\"{}\"}}",
            "x".repeat(1_048_577)
        )
        .into_bytes(),
    ] {
        std::fs::write(root.path().join("reply"), bytes).unwrap();
        assert!(latest_using(exe.as_os_str()).await.is_none());
    }
    assert!(latest_using(OsStr::new("blent-t696-absent-curl"))
        .await
        .is_none());
    for limit in [0, 16_777_217, usize::MAX] {
        assert!(
            crate::command_output::read(exe.as_os_str(), &[], limit, Duration::from_secs(1))
                .await
                .is_err()
        );
    }
    assert!(
        crate::command_output::read(exe.as_os_str(), &[], 1, Duration::ZERO)
            .await
            .is_err()
    );
    std::fs::write(root.path().join("mode"), "hang").unwrap();
    assert!(
        crate::command_output::read(exe.as_os_str(), &[], 1024, Duration::from_millis(100))
            .await
            .is_err()
    );
}
#[tokio::test]
async fn t696_shutdown_and_drop_retire_inflight_owned_process() {
    for explicit in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let exe = fixture(root.path());
        std::fs::write(root.path().join("mode"), "hang").unwrap();
        let subscription = Subscription::start(true, exe.into());
        advance(FIRST_CHECK).await;
        wait(&format!("start explicit={explicit}"), || {
            std::fs::read_to_string(root.path().join("started"))
                .is_ok_and(|s| s.parse::<u32>().is_ok())
        })
        .await;
        let pid: u32 = std::fs::read_to_string(root.path().join("started"))
            .unwrap()
            .parse()
            .unwrap();
        if explicit {
            subscription.shutdown().await;
        } else {
            drop(subscription);
        }
        wait(&format!("retire pid={pid} explicit={explicit}"), || {
            !alive(pid)
        })
        .await;
    }
}
#[cfg(windows)]
fn alive(pid: u32) -> bool {
    blent_config::windows::process::Identity::read(pid).is_ok()
}
#[cfg(target_os = "linux")]
fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/status")).is_ok_and(|s| !s.contains("State:\tZ"))
}
