// T232: exercise each production consumer with the same isolated PATH.
pub fn check(lookup: fn(&str) -> bool, test_name: &str) {
    if std::env::var_os("USCREEN_T232_CHILD").is_some() {
        for (name, expected) in [
            ("adb", true), ("shadowed", true), ("directory", false),
            ("plain", false), ("missing", false), ("which", false),
        ] {
            assert_eq!(lookup(name), expected, "T232: {name}");
        }
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first [tools]");
    let second = root.path().join("second café");
    std::fs::create_dir_all(first.join("directory")).unwrap();
    std::fs::create_dir(&second).unwrap();
    for name in ["plain", "shadowed"] {
        std::fs::write(first.join(name), "not executable").unwrap();
    }
    use std::os::unix::fs::PermissionsExt;
    for name in ["adb", "shadowed"] {
        let path = second.join(name);
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture"])
        .env("USCREEN_T232_CHILD", "1")
        .env("PATH", std::env::join_paths([first, second]).unwrap())
        .output().unwrap();
    assert!(result.status.success(), "T232: {}{}",
        String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
}
