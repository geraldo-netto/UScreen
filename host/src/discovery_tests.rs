//! T390: deterministic device stalls must not block unrelated tablets.
use super::*;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

fn fake_adb(root: &std::path::Path) -> PathBuf {
    let path = root.join("adb");
    std::fs::write(
        &path,
        r#"#!/bin/sh
if [ "$2" = SLOW ] && [ "$3" = shell ]; then
    printf ready > "$0.slow-started"
    while [ ! -e "$0.release" ]; do sleep 0.01; done
fi
cat >/dev/null
printf launched > "$0.$2.launched"
"#,
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

async fn wait_file(path: &std::path::Path) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("T390: fake ADB was never reached");
}

#[tokio::test]
async fn t390_slow_token_delivery_cannot_delay_another_tablet() {
    let root = tempfile::tempdir().unwrap();
    let adb = fake_adb(root.path());
    let command = adb.to_str().unwrap().to_owned();
    let task = tokio::spawn(async move {
        monitor::test_support::deliver_assigned_tokens(
            &["SLOW".into(), "FAST".into()],
            None,
            &command,
        )
        .await;
    });
    wait_file(&adb.with_extension("slow-started")).await;
    let fast = tokio::time::timeout(
        Duration::from_millis(250),
        wait_file(&adb.with_extension("FAST.launched")),
    )
    .await
    .is_ok();
    // Release the gate and join even on the pre-fix failure path.
    std::fs::write(adb.with_extension("release"), "").unwrap();
    task.await.unwrap();
    assert!(
        fast,
        "T390: FAST token delivery waited for SLOW's ADB response"
    );
}

#[tokio::test]
async fn t390_device_jobs_complete_independently_and_bound_concurrency() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let mut tasks = device_tasks::DeviceTasks::new(2);
    let (release, hold) = tokio::sync::oneshot::channel();
    let third_started = Arc::new(AtomicBool::new(false));
    assert!(tasks.schedule("SLOW".into(), async move {
        let _ = hold.await;
        1
    }));
    assert!(tasks.schedule("FAST".into(), async { 2 }));
    let third = third_started.clone();
    assert!(tasks.schedule("THIRD".into(), async move {
        third.store(true, Ordering::Release);
        3
    }));
    assert!(!tasks.schedule("SLOW".into(), async { 99 }));
    assert!(!third_started.load(Ordering::Acquire));
    let (serial, value) = tokio::time::timeout(Duration::from_millis(250), tasks.next())
        .await
        .unwrap();
    assert_eq!((serial.as_str(), value.unwrap()), ("FAST", 2));
    let (serial, value) = tokio::time::timeout(Duration::from_millis(250), tasks.next())
        .await
        .unwrap();
    assert_eq!((serial.as_str(), value.unwrap()), ("THIRD", 3));
    assert!(third_started.load(Ordering::Acquire));
    release.send(()).unwrap();
    assert_eq!(tasks.next().await.1.unwrap(), 1);
    tasks.stop().await;
}

#[tokio::test]
async fn t390_device_jobs_cancel_without_ever_starting_queued_work() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let mut tasks = device_tasks::DeviceTasks::new(1);
    let queued_started = Arc::new(AtomicBool::new(false));
    tasks.schedule("SLOW".into(), std::future::pending::<()>());
    let started = queued_started.clone();
    tasks.schedule("QUEUED".into(), async move {
        started.store(true, Ordering::Release);
    });
    tasks.cancel("QUEUED");
    tasks.cancel("SLOW");
    let (_, result) = tokio::time::timeout(Duration::from_millis(250), tasks.next())
        .await
        .unwrap();
    assert!(result.unwrap_err().is_cancelled());
    assert!(!tasks.contains("SLOW") && !tasks.contains("QUEUED"));
    assert!(!queued_started.load(Ordering::Acquire));
    tasks.stop().await;
}

fn isolated_monitor_test(name: &str) -> Option<tempfile::TempDir> {
    if std::env::var_os("USCREEN_T390_ROOT").is_some() {
        unsafe {
            libc::alarm(15);
            assert_eq!(libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0), 0);
        }
        return None;
    }
    let root = tempfile::tempdir().unwrap();
    // T435: production rejects shared runtime directories, regardless of umask.
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(root.path().join("runtime"))
        .unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("discovery_tests::{name}"),
            "--nocapture",
        ])
        .env("USCREEN_T390_ROOT", root.path())
        .env("HOME", root.path())
        .env("XDG_RUNTIME_DIR", root.path().join("runtime"))
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env_remove("USCREEN_FAKE_TABLET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
    Some(root)
}

#[test]
fn t435_shared_umask_keeps_monitor_fixture_private() {
    use std::os::unix::process::CommandExt;
    if std::env::var_os("USCREEN_T435_CHILD").is_none() {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "discovery_tests::t435_shared_umask_keeps_monitor_fixture_private",
            ])
            .env("USCREEN_T435_CHILD", "1");
        unsafe {
            command.pre_exec(|| {
                libc::umask(0o002);
                Ok(())
            });
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "T435: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    if isolated_monitor_test("t435_shared_umask_keeps_monitor_fixture_private").is_none() {
        runtime::runtime_dir().expect("T435: fixture must meet production runtime permissions");
    }
}

fn discovery_adb(root: &std::path::Path) -> PathBuf {
    let path = root.join("adb");
    std::fs::write(&path, r#"#!/bin/sh
if [ "$1" = devices ]; then printf 'List of devices attached\nSLOW\tdevice\nFAST\tdevice\n'; exit 0; fi
if [ "$4" = getprop ]; then
    if [ "$2" = SLOW ]; then
        printf ready > "$0.slow-started"
        while [ ! -e "$0.release" ]; do sleep 0.01; done
    else
        printf ready > "$0.fast-probed"
    fi
    echo "$2-identity"
fi
if [ "$4" = pm ]; then echo package:/app/uscreen.apk; fi
if [ "$3" = reverse ]; then printf '%s\n' "$*" >> "$0.reverse"; fi
exit 0
"#).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[tokio::test]
async fn t390_initial_discovery_connects_fast_tablet_before_slow_probe() {
    if isolated_monitor_test("t390_initial_discovery_connects_fast_tablet_before_slow_probe")
        .is_some()
    {
        return;
    }
    let root = PathBuf::from(std::env::var_os("USCREEN_T390_ROOT").unwrap());
    let adb = discovery_adb(&root);
    let (mode, _) = watch::channel(false);
    let (stop, stop_rx) = watch::channel(false);
    let capture = capture::CaptureConfig {
        helper_path: "/missing-t390-helper".into(),
        ..Default::default()
    };
    let tablet = session::Spec {
        capture: capture.clone(),
        ports: (18000, 18001),
        token: None,
        devices: (false, false, false),
    }
    .prepare(mode.clone())
    .tablet;
    let mut attached = tablet.subscribe();
    let extra = ExtraSessionTemplate {
        max_tablets: 1,
        cap_template: capture,
        video_port: 18000,
        input_port: 18001,
        token: None,
        input_touch: false,
        input_pen: false,
        input_pointer: false,
        mode_tx: mode,
        shutdown_rx: stop_rx,
    };
    let command = adb.to_str().unwrap().to_owned();
    let task = tokio::spawn(async move {
        adb_monitor_using(
            18000,
            18001,
            false,
            tablet,
            None,
            Default::default(),
            extra,
            &command,
        )
        .await;
    });
    wait_file(&adb.with_extension("fast-probed")).await;
    let fast = tokio::time::timeout(Duration::from_millis(250), async {
        while !*attached.borrow_and_update() {
            attached.changed().await.unwrap();
        }
    })
    .await
    .is_ok();
    std::fs::write(adb.with_extension("release"), "").unwrap();
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert!(
        fast,
        "T390: ready FAST tablet waited for unrelated SLOW identity probe"
    );
    let forwards = std::fs::read_to_string(adb.with_extension("reverse")).unwrap();
    assert!(forwards.contains("-s FAST reverse tcp:8890 tcp:18000"));
}

pub(crate) fn monitor_inputs(
    max_tablets: u32,
    ports: (u16, u16),
) -> (
    attachment::Attachment,
    ExtraSessionTemplate,
    watch::Sender<bool>,
) {
    let (mode, _) = watch::channel(false);
    let (stop, shutdown_rx) = watch::channel(false);
    let capture = capture::CaptureConfig {
        helper_path: "/missing-t390-helper".into(),
        ..Default::default()
    };
    let tablet = session::Spec {
        capture: capture.clone(),
        ports,
        token: None,
        devices: (false, false, false),
    }
    .prepare(mode.clone())
    .tablet;
    let extra = ExtraSessionTemplate {
        max_tablets,
        cap_template: capture,
        video_port: ports.0,
        input_port: ports.1,
        token: None,
        input_touch: false,
        input_pen: false,
        input_pointer: false,
        mode_tx: mode,
        shutdown_rx,
    };
    (tablet, extra, stop)
}

#[derive(serde::Deserialize)]
struct PublishedSessions {
    sessions: Vec<runtime::TabletSession>,
}

async fn wait_sessions(expected: usize) -> Vec<runtime::TabletSession> {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            // The fixture executable is a Cargo test binary, not a running
            // daemon. Inspect this owner's snapshot; PID validation has T135.
            let sessions = std::fs::read(runtime::runtime_dir().unwrap().join("sessions.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<PublishedSessions>(&bytes).ok())
                .map(|snapshot| snapshot.sessions)
                .unwrap_or_default();
            if sessions.len() == expected {
                return sessions;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("T390: independently ready devices never became active")
}

fn scaling_adb(root: &std::path::Path, count: u32) -> PathBuf {
    let path = discovery_adb(root);
    let source = std::fs::read_to_string(&path).unwrap();
    let devices = (1..=count)
        .map(|id| format!("FAST{id}\\tdevice\\n"))
        .collect::<String>();
    let source = source.replace("FAST\\tdevice\\n", &devices).replace(
        "printf ready > \"$0.slow-started\"",
        "echo $$ > \"$0.slow-started\"",
    );
    std::fs::write(&path, source).unwrap();
    path
}

async fn scaling_case(root: &std::path::Path, count: u32) {
    let folder = root.join(format!("scale-{count}"));
    std::fs::create_dir(&folder).unwrap();
    let adb = scaling_adb(&folder, count);
    let video_port = 20000 + (std::process::id() % 4000) as u16 * 8;
    let ports = (video_port, video_port + 1);
    let (tablet, extra, stop) = monitor_inputs(count, ports);
    let command = adb.to_str().unwrap().to_owned();
    let start = std::time::Instant::now();
    let task = tokio::spawn(async move {
        adb_monitor_using(
            ports.0,
            ports.1,
            false,
            tablet,
            None,
            Default::default(),
            extra,
            &command,
        )
        .await;
    });
    wait_file(&adb.with_extension("slow-started")).await;
    let sessions = wait_sessions(count as usize).await;
    let connected = start.elapsed();
    assert!(sessions
        .iter()
        .all(|session| session.serial.starts_with("FAST")));
    assert_eq!(
        sessions.iter().map(|s| s.instance).collect::<Vec<_>>(),
        (0..count).collect::<Vec<_>>()
    );
    let reverse = std::fs::read_to_string(adb.with_extension("reverse")).unwrap();
    for session in &sessions {
        assert_forward_pair(&reverse, session);
    }
    let stop_start = std::time::Instant::now();
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_millis(500), task)
        .await
        .unwrap()
        .unwrap();
    let stopped = stop_start.elapsed();
    tokio::time::sleep(Duration::from_millis(50)).await;
    // Reap adopted fake shell descendants after owned command cancellation.
    unsafe { while libc::waitpid(-1, std::ptr::null_mut(), 0) > 0 {} }
    let pid = std::fs::read_to_string(adb.with_extension("slow-started")).unwrap();
    assert!(!std::path::Path::new(&format!("/proc/{}", pid.trim())).exists());
    assert!(
        !adb.with_extension("release").exists(),
        "slow device must remain gated through shutdown"
    );
    println!(
        "T390 scaling clients={count} connect_us={} stop_us={}",
        connected.as_micros(),
        stopped.as_micros()
    );
}

fn assert_forward_pair(log: &str, session: &runtime::TabletSession) {
    let entries: Vec<_> = log
        .lines()
        .filter(|line| line.starts_with(&format!("-s {} ", session.serial)))
        .collect();
    assert_eq!(
        entries,
        [
            format!(
                "-s {} reverse tcp:8890 tcp:{}",
                session.serial, session.video_port
            ),
            format!(
                "-s {} reverse tcp:8891 tcp:{}",
                session.serial, session.input_port
            )
        ]
    );
}

#[tokio::test]
async fn t390_one_two_four_tablets_connect_and_stop_while_probe_is_stalled() {
    if isolated_monitor_test("t390_one_two_four_tablets_connect_and_stop_while_probe_is_stalled")
        .is_some()
    {
        return;
    }
    let root = PathBuf::from(std::env::var_os("USCREEN_T390_ROOT").unwrap());
    for count in [1, 2, 4] {
        scaling_case(&root, count).await;
    }
}
