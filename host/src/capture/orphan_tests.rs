use super::*;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

pub(super) fn fixture_programs(root: &std::path::Path) {
    let source = root.join("fixture.c");
    std::fs::write(
        &source,
        r#"
#include <stdio.h>
#include <stdlib.h>
#include <signal.h>
#include <unistd.h>
int main(void) {
    if (getenv("BLENT_IGNORE_TERM")) signal(SIGTERM, SIG_IGN);
    puts("ready"); fflush(stdout);
    for (;;) pause();
}
"#,
    )
    .unwrap();
    let output = std::process::Command::new("cc")
        .arg(&source)
        .arg("-o")
        .arg(root.join("ffmpeg"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for name in ["evdi_helper", "unrelated"] {
        std::fs::copy(root.join("ffmpeg"), root.join(name)).unwrap();
    }
}

pub(super) async fn fixture_child(
    program: &std::path::Path,
    flag: &str,
    fifo: &std::path::Path,
    ignore_term: bool,
) -> Child {
    let mut command = Command::new(program);
    command
        .args([std::ffi::OsStr::new(flag), fifo.as_os_str()])
        .stdout(Stdio::piped())
        .kill_on_drop(true);
    if ignore_term {
        command.env("BLENT_IGNORE_TERM", "1");
    }
    let mut child = command.spawn().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("ready"));
    child
}

#[tokio::test]
async fn t245_retirement_waits_for_term_resistant_children() {
    let dir = tempfile::tempdir().unwrap();
    fixture_programs(dir.path());
    let fifo = dir.path().join("capture.fifo");
    let mut child = fixture_child(&dir.path().join("ffmpeg"), "-i", &fifo, true).await;
    let start = Instant::now();
    process::retire_orphan_capture(&fifo).await.unwrap();
    let elapsed = start.elapsed();
    assert!(elapsed >= std::time::Duration::from_millis(1500));
    assert!(
        elapsed < std::time::Duration::from_secs(3),
        "T245: retirement exceeded its budget"
    );
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(child.wait().await.unwrap().signal(), Some(libc::SIGKILL));
}

#[tokio::test]
async fn t245_changed_or_foreign_process_identity_is_never_signalled() {
    use blent_config::linux::processes::{retire, Process};
    let dir = tempfile::tempdir().unwrap();
    fixture_programs(dir.path());
    let fifo = dir.path().join("capture.fifo");
    let mut child = fixture_child(&dir.path().join("ffmpeg"), "-i", &fifo, false).await;
    let process = Process::read(child.id().unwrap()).unwrap();
    let mut stale = process.clone();
    stale.start_ticks += 1;
    let budget = std::time::Duration::from_millis(20);
    assert_eq!(retire(&[stale], budget, budget).await.unwrap(), 0);
    let mut foreign = process.clone();
    foreign.uid = foreign.uid.wrapping_add(1);
    assert!(retire(&[foreign], budget, budget).await.is_err());
    assert!(child.try_wait().unwrap().is_none());
    assert_eq!(retire(&[process], budget, budget).await.unwrap(), 1);
    child.wait().await.unwrap();
}

#[tokio::test]
async fn t245_orphan_cleanup_matches_literal_fifo_arguments_and_programs() {
    let dir = tempfile::tempdir().unwrap();
    fixture_programs(dir.path());
    let fifo = dir.path().join("runtime space [1]/capture.fifo");
    let regex_neighbor = dir.path().join("runtime space 1/captureXfifo");
    let prefix_neighbor = fifo.with_extension("fifo-other");
    let cases = [
        ("evdi_helper", "--capture-fifo", &fifo, true),
        ("ffmpeg", "-i", &fifo, true),
        ("evdi_helper", "--capture-fifo", &regex_neighbor, false),
        ("ffmpeg", "-i", &regex_neighbor, false),
        ("evdi_helper", "--capture-fifo", &prefix_neighbor, false),
        ("ffmpeg", "-i", &prefix_neighbor, false),
        ("ffmpeg", "-metadata", &fifo, false),
        ("unrelated", "--capture-fifo", &fifo, false),
    ];
    let mut children = Vec::new();
    for (program, flag, path, _) in &cases {
        children.push(fixture_child(&dir.path().join(program), flag, path, false).await);
    }
    process::retire_orphan_capture(&fifo).await.unwrap();
    for (child, (_, _, _, retired)) in children.iter_mut().zip(cases) {
        assert_eq!(
            child.try_wait().unwrap().is_some(),
            retired,
            "T245: wrong process selected: {:?}",
            child.id()
        );
    }
    for child in &mut children {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
}
