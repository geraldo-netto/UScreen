//! T497: update polling publishes only validated releases and retains state on failure.
use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

const TEST: &str =
    "update::coverage_tests::t497_polling_retains_last_release_during_network_failure";

fn isolated() {
    let directory = tempfile::tempdir().unwrap();
    let tool = directory.path().join("curl");
    std::fs::write(&tool, "#!/bin/sh\n[ ! -f \"$BLENT_T497_UPDATE/fail\" ] || exit 42\n/bin/cat \"$BLENT_T497_UPDATE/reply\"\n").unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700)).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture"])
        .env("PATH", directory.path())
        .env("BLENT_T497_UPDATE", directory.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn advance(duration: Duration) {
    tokio::time::pause();
    tokio::task::yield_now().await;
    tokio::time::advance(duration).await;
    tokio::time::resume();
}

async fn receive(rx: &mut watch::Receiver<Available>) -> Available {
    tokio::time::timeout(Duration::from_secs(2), rx.changed())
        .await
        .unwrap()
        .unwrap();
    rx.borrow_and_update().clone()
}

async fn malformed(root: &Path) {
    for body in [
        "",
        "not json",
        "[]",
        "{\"tag_name\":42}",
        "{\"tag_name\":null}",
    ] {
        std::fs::write(root.join("reply"), body).unwrap();
        assert!(latest_release_tag().await.is_none(), "T497 accepted {body}");
    }
    std::fs::write(root.join("fail"), "").unwrap();
    assert!(latest_release_tag().await.is_none());
    std::fs::remove_file(root.join("fail")).unwrap();
}

#[tokio::test]
async fn t497_polling_retains_last_release_during_network_failure() {
    let Ok(directory) = std::env::var("BLENT_T497_UPDATE") else {
        isolated();
        return;
    };
    let root = Path::new(&directory);
    malformed(root).await;
    std::fs::write(root.join("reply"), "{\"tag_name\":\"v999.0.0\"}").unwrap();
    let (tx, mut rx) = watch::channel(None);
    let task = tokio::spawn(run(tx));
    advance(FIRST_CHECK).await;
    assert_eq!(receive(&mut rx).await.as_deref(), Some("999.0.0"));
    std::fs::write(root.join("fail"), "").unwrap();
    advance(INTERVAL).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(100), rx.changed())
            .await
            .is_err()
    );
    assert_eq!(rx.borrow().as_deref(), Some("999.0.0"));
    std::fs::remove_file(root.join("fail")).unwrap();
    std::fs::write(
        root.join("reply"),
        format!("{{\"tag_name\":\"{}\"}}", current_version()),
    )
    .unwrap();
    advance(INTERVAL).await;
    assert_eq!(receive(&mut rx).await, None);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}
