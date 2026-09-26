use super::*;

#[test]
fn t582_requests_cover_owned_groups_without_boosting_an_editor() {
    for unit in [
        include_str!("../../../../scripts/blent.service"),
        include_str!("../../../../packaging/blent.service"),
    ] {
        assert!(unit.lines().any(|line| line == "CPUWeight=1000"));
        assert!(!unit.contains("CPUSchedulingPolicy=fifo"));
    }
    for (priority, expected) in [(Priority::Normal, 100), (Priority::High, 1000)] {
        for unit in ["blent.service", "blent-priority-123.scope"] {
            request(
                123,
                priority,
                &format!("0::/user/app.slice/{unit}\n"),
                |program, args| {
                    assert_eq!(program, "systemctl");
                    assert_eq!(
                        args,
                        [
                            "--user",
                            "set-property",
                            "--runtime",
                            unit,
                            &format!("CPUWeight={expected}")
                        ]
                    );
                    Ok(())
                },
            )
            .unwrap();
        }
        request(
            123,
            priority,
            "0::/user/app.slice/app-code.scope",
            |program, args| {
                assert_eq!(program, "busctl");
                assert_eq!(args, scope_arguments(123, expected));
                assert!(!args.iter().any(|arg| arg == "app-code.scope"));
                assert!(args.windows(4).any(|v| v == ["PIDs", "au", "1", "123"]));
                Ok(())
            },
        )
        .unwrap();
    }
    assert!(owned_unit("/user/blent-priority-124.scope", 123).is_none());
    assert!(owned_unit("/user/blent.service/other", 123).is_none());
    assert!(request(
        123,
        Priority::High,
        "0::/user/blent.service",
        |_, _| anyhow::bail!("denied")
    )
    .is_err());
    assert!(run("/bin/true", &[]).is_ok());
    assert!(run("/bin/false", &[]).is_err());
    let directory = tempfile::tempdir().unwrap();
    assert!(run(directory.path().join("missing").to_str().unwrap(), &[]).is_err());
}

#[test]
fn t582_group_input_and_effective_weight_are_validated() {
    let root = tempfile::tempdir().unwrap();
    let group = root.path().join("user/blent.service");
    std::fs::create_dir_all(&group).unwrap();
    let text = "0::/user/blent.service\n";
    assert!(verify(root.path(), text, Priority::High).is_err());
    for value in ["", "oops", "0", "100", "10000"] {
        std::fs::write(group.join("cpu.weight"), value).unwrap();
        assert!(verify(root.path(), text, Priority::High).is_err());
    }
    for (priority, value) in [(Priority::High, "1000\n"), (Priority::Normal, "100\n")] {
        std::fs::write(group.join("cpu.weight"), value).unwrap();
        assert!(verify(root.path(), text, priority)
            .unwrap()
            .contains(value.trim()));
    }
    for invalid in ["", "1:cpu:/x", "0::relative", "0::/../x", "0::/a/../x"] {
        assert!(group_path(invalid).is_err());
        assert!(request(1, Priority::High, invalid, |_, _| panic!(
            "T582 invalid input reached systemd"
        ))
        .is_err());
    }
    for length in 0..128 {
        let input = "x".repeat(length);
        assert!(group_path(&input).is_err());
    }
}

#[test]
fn t582_async_scope_start_is_verified_with_a_bounded_wait() {
    let mut attempts = 0;
    let result = wait_effective(Duration::from_secs(1), || {
        attempts += 1;
        if attempts == 1 {
            anyhow::bail!("still in old group")
        }
        Ok("effective".into())
    });
    assert_eq!(result.unwrap(), "effective");
    assert_eq!(attempts, 2);
    assert!(wait_effective(Duration::ZERO, || anyhow::bail!("denied")).is_err());
}

#[test]
fn t582_only_the_same_user_and_exact_adb_server_are_eligible() {
    let mut process = Process {
        pid: u32::MAX,
        uid: 123,
        start_ticks: 10,
        executable: "/opt/tools/adb".into(),
        arguments: ["adb", "-L", "tcp:5037", "fork-server", "server"]
            .map(Into::into)
            .into(),
        cwd: "/".into(),
    };
    let executable = Path::new("/opt/tools/adb");
    assert!(shared_server(&process, executable, 123));
    assert!(!shared_server(&process, executable, 124));
    assert!(!shared_server(&process, Path::new("/other/adb"), 123));
    process.uid = unsafe { libc::geteuid() };
    assert!(apply_server(&process, executable, Priority::High).is_err());
    process.arguments[2] = "tcp:5038".into();
    assert!(!shared_server(&process, executable, process.uid));
    assert!(apply_server(&process, executable, Priority::High).is_ok());
    process.arguments.clear();
    assert!(!shared_server(&process, executable, process.uid));
}

#[test]
fn t582_native_scope_keeps_all_threads_and_new_children_together() {
    const CHILD: &str = "BLENT_T582_ISOLATED_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let directory = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", directory.path());
        // An unavailable service manager is an explicit, nonfatal capability
        // result on CI. Pure tests cover failure; this machine also runs native.
        if let Err(error) = apply_current(Priority::High) {
            eprintln!("T582 native systemd unavailable: {error:#}");
            return;
        }
        assert_eq!(
            crate::scheduling::apply_configured(),
            "This process: High priority active"
        );
        assert!(apply_current(Priority::Normal)
            .unwrap()
            .contains("CPUWeight=100"));
        let group = std::fs::read_to_string("/proc/self/cgroup").unwrap();
        std::thread::spawn({
            let group = group.clone();
            move || {
                assert_eq!(
                    std::fs::read_to_string("/proc/thread-self/cgroup").unwrap(),
                    group
                );
            }
        })
        .join()
        .unwrap();
        let output = Command::new("cat")
            .arg("/proc/self/cgroup")
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(output.stdout).unwrap(), group);
        isolated_adb(directory.path());
        return;
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "scheduling::linux::tests::t582_native_scope_keeps_all_threads_and_new_children_together", "--nocapture"])
        .env(CHILD, "1").output_timeout(Duration::from_secs(15)).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
}

fn isolated_adb(directory: &Path) {
    let executable = directory.join("adb");
    std::fs::copy("/bin/bash", &executable).unwrap();
    // T582: bundled adb shadows the SDK executable that owns the live server.
    let bundled = directory.join("bundled");
    std::fs::create_dir(&bundled).unwrap();
    std::fs::copy("/bin/true", bundled.join("adb")).unwrap();
    for program in ["busctl", "systemctl"] {
        std::os::unix::fs::symlink(format!("/usr/bin/{program}"), directory.join(program)).unwrap();
    }
    // Never expose the real host ADB executable to this isolated regression.
    let paths = [bundled.clone(), directory.to_path_buf()];
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    let mut child = Command::new(&executable)
        .args([
            "-c",
            "printf ready; read -r -t 10 ignored",
            "fork-server",
            "server",
            "-L",
            "tcp:5037",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = [0; 5];
    std::io::Read::read_exact(child.stdout.as_mut().unwrap(), &mut ready).unwrap();
    assert_eq!(&ready, b"ready");
    assert!(
        child.try_wait().unwrap().is_none(),
        "T582 server fixture exited before discovery"
    );
    let identity = Process::read(child.id()).expect("T582 owned server fixture exists");
    assert!(
        shared_server(&identity, &executable.canonicalize().unwrap(), unsafe {
            libc::geteuid()
        }),
        "T582 fixture identity: {identity:?}"
    );
    assert_eq!(
        crate::linux::programs::find_in("adb", &std::env::var_os("PATH").unwrap()).unwrap(),
        bundled.join("adb")
    );
    let result = apply_shared_adb(Priority::High);
    let group = std::fs::read_to_string(format!("/proc/{}/cgroup", child.id())).unwrap();
    let _ = child.kill();
    let _ = child.wait();
    result.unwrap();
    assert!(
        group.contains(&format!("blent-priority-{}.scope", child.id())),
        "T582 fixture remained in {group}"
    );
    std::env::set_var("PATH", directory.join("missing"));
    crate::scheduling::apply_shared_adb(Priority::Normal);
    // The application must also survive a denied/missing native manager.
    assert_eq!(
        crate::scheduling::apply_configured(),
        "This process: High priority unavailable; using OS settings"
    );
}
