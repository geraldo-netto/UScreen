//! T529: native streaming pipes and owned process cancellation on every host.
#![cfg(feature = "commands")]
use blent_config::commands::OwnedChild;
use std::{
    io::{Read, Write},
    path::Path,
    process::Stdio,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn fixture(role: &str, root: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "t529_pipe_child", "--nocapture"])
        .env("BLENT_T529_ROLE", role)
        .env("BLENT_T529_ROOT", root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command
}

#[test]
fn t529_pipe_child() {
    let Ok(role) = std::env::var("BLENT_T529_ROLE") else {
        return;
    };
    let root = std::path::PathBuf::from(std::env::var_os("BLENT_T529_ROOT").unwrap());
    match role.as_str() {
        "echo" => {
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes).unwrap();
            std::io::stdout().write_all(&bytes).unwrap();
        }
        "parent" => {
            let mut child = fixture("worker", &root).as_std_mut().spawn().unwrap();
            child.wait().unwrap();
        }
        "worker" => {
            std::fs::write(root.join("ready"), b"ready").unwrap();
            std::thread::sleep(Duration::from_secs(3));
            std::fs::write(root.join("escaped"), b"escaped").unwrap();
        }
        _ => panic!("T529: unknown child role"),
    }
    std::process::exit(0);
}

#[tokio::test]
async fn t529_owned_stream_preserves_large_input_output_and_eof() {
    let root = tempfile::tempdir().unwrap();
    let mut child = OwnedChild::spawn(&mut fixture("echo", root.path())).unwrap();
    assert!(child.id().is_some());
    let mut stdin = child.take_stdin().unwrap();
    let mut stdout = child.take_stdout().unwrap();
    assert!(child.take_stdin().is_none());
    assert!(child.take_stdout().is_none());
    let input = vec![0xa5; 1024 * 1024];
    let mut output = Vec::new();
    let (write, read) = tokio::join!(
        async {
            stdin.write_all(&input).await?;
            stdin.shutdown().await?;
            drop(stdin);
            Ok::<_, std::io::Error>(())
        },
        stdout.read_to_end(&mut output)
    );
    write.unwrap();
    read.unwrap();
    assert!(output.ends_with(&input));
    assert!(child
        .finish(Duration::from_secs(2))
        .await
        .unwrap()
        .success());
}

async fn ready(root: &Path) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !root.join("ready").exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("T529: child never became ready");
}

#[tokio::test]
async fn t529_owned_stream_drop_timeout_and_cancel_retire_descendants() {
    for retirement in 0..3 {
        let root = tempfile::tempdir().unwrap();
        let child = OwnedChild::spawn(&mut fixture("parent", root.path())).unwrap();
        ready(root.path()).await;
        match retirement {
            0 => drop(child),
            1 => assert_eq!(
                child
                    .finish(Duration::from_millis(1))
                    .await
                    .unwrap_err()
                    .kind(),
                std::io::ErrorKind::TimedOut
            ),
            _ => {
                let task = tokio::spawn(child.finish(Duration::from_secs(60)));
                tokio::task::yield_now().await;
                task.abort();
                assert!(task.await.unwrap_err().is_cancelled());
            }
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(
            !root.path().join("escaped").exists(),
            "T529: descendant escaped retirement {retirement}"
        );
    }
}

#[tokio::test]
async fn t529_owned_stream_reports_start_failure() {
    let root = tempfile::tempdir().unwrap();
    let mut command = tokio::process::Command::new(root.path().join("missing-encoder"));
    assert!(OwnedChild::spawn(&mut command).is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
