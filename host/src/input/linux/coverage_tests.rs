//! T497: real constructor/retirement paths with a fail-closed private uinput shim.
use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

const TEST: &str =
    "input::linux::coverage_tests::t497_uinput_construction_errors_and_owned_retirement";

fn isolated() {
    let directory = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let root = directory.path();
    let library = root.join("uinput-mock.so");
    let compiler = std::process::Command::new("cc")
        .args(["-shared", "-fPIC", "-Wall", "-Wextra", "-Werror"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/uinput_mock.c"))
        .arg("-o")
        .arg(&library)
        .output()
        .unwrap();
    assert!(
        compiler.status.success(),
        "{}",
        String::from_utf8_lossy(&compiler.stderr)
    );
    for failure in [
        0,
        UI_SET_EVBIT,
        UI_ABS_SETUP,
        UI_DEV_SETUP,
        UI_DEV_CREATE,
        u64::MAX,
    ] {
        let device = root.join(format!("device-{failure}"));
        if failure != u64::MAX {
            std::fs::write(&device, "").unwrap();
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture"])
            .env("LD_PRELOAD", &library)
            .env("USCREEN_T497_UINPUT_FILE", &device)
            .env(
                "USCREEN_T497_UINPUT_LOG",
                root.join(format!("ioctl-{failure}")),
            )
            .env("USCREEN_T497_UINPUT_FAIL", failure.to_string())
            .env("PATH", root)
            .env("HOME", root)
            .env("XDG_CONFIG_HOME", root)
            .env("XDG_RUNTIME_DIR", root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "failure {failure}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn assert_intercepted() {
    let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"uscreen_test_uinput_active".as_ptr()) };
    assert!(
        !symbol.is_null(),
        "T497 refuses device constructors without the syscall shim"
    );
    let probe: unsafe extern "C" fn() -> i32 = unsafe { std::mem::transmute(symbol) };
    assert_eq!(unsafe { probe() }, 497);
}

fn construction_contract() {
    let names = DeviceIdentity::for_instance(127);
    for flags in 0..8 {
        let cfg = InputConfig {
            touch: flags & 1 != 0,
            pen: flags & 2 != 0,
            pointer: flags & 4 != 0,
            ..Default::default()
        };
        let devices = InjectDevices::create(&cfg, &names);
        assert_eq!(devices.touch.is_some(), cfg.touch);
        assert_eq!(devices.pen.is_some(), cfg.pen);
        assert_eq!(devices.pointer.is_some(), cfg.pen && cfg.pointer);
    }
    let long_name = "x".repeat(160);
    drop(UInputDevice::new_pointer(&long_name, u16::MAX).unwrap());
    let log =
        std::fs::read_to_string(std::env::var_os("USCREEN_T497_UINPUT_LOG").unwrap()).unwrap();
    assert!(log.contains("axis 0 0 65535 211"));
    assert!(log.contains("axis 47 0 9 0"));
    assert!(log.contains("axis 26 -1571 1571 1000"));
    assert!(log.contains("device 6 17747 65535 1 79"));
    assert_eq!(
        log.matches(&format!("request {UI_DEV_CREATE}\n")).count(),
        log.matches(&format!("request {UI_DEV_DESTROY}\n")).count()
    );
}

async fn owner_contract() {
    let devices = Arc::new(std::sync::Mutex::new(InjectDevices::empty()));
    let mut owner = DeviceOwner {
        devices: devices.clone(),
        touch_registered: false,
    };
    let cfg = InputConfig::default();
    assert_eq!(
        owner
            .create_devices(&cfg, &DeviceIdentity::for_instance(0))
            .await,
        3
    );
    assert_eq!(TOUCH_DEVICES.load(Ordering::SeqCst), 1);
    for (pen_only, card) in [(false, None), (true, Some(17))] {
        devices
            .lock()
            .unwrap()
            .inject_touch(100, 200, 300, 0, 0)
            .unwrap();
        assert_eq!(owner.release_for_remap(pen_only, card), 3);
        assert_eq!(devices.lock().unwrap().active_slots, 0);
    }
    owner.remove_devices().await;
    assert_eq!(TOUCH_DEVICES.load(Ordering::SeqCst), 0);
    assert_eq!(owner.release_for_remap(false, None), 0);
    owner
        .create_devices(&cfg, &DeviceIdentity::for_instance(0))
        .await;
    drop(owner);
    tokio::task::yield_now().await;
    assert_eq!(TOUCH_DEVICES.load(Ordering::SeqCst), 0);
    assert_eq!(devices.lock().unwrap().count(), 0);
}

#[tokio::test]
async fn t497_uinput_construction_errors_and_owned_retirement() {
    let Ok(failure) = std::env::var("USCREEN_T497_UINPUT_FAIL") else {
        isolated();
        return;
    };
    assert_intercepted();
    crate::test_logging::enable();
    if failure != "0" {
        let error = match UInputDevice::new_pointer("fixture", 3) {
            Ok(_) => panic!("T497 constructor ignored the injected syscall failure"),
            Err(error) => error,
        };
        assert!(!error.to_string().is_empty());
        assert!(
            create_device(true, "failed", || UInputDevice::new_pointer("fixture", 3)).is_none()
        );
        return;
    }
    construction_contract();
    owner_contract().await;
    diagnostic_contract();
    for fd in [-1, i32::MIN, i32::MAX] {
        assert!(unsafe { UInputDevice::ioctl_val(fd, UI_SET_EVBIT, 0) }.is_err());
        assert!(unsafe { UInputDevice::dev_setup_and_create(fd, "", 0) }.is_err());
    }
}

fn diagnostic_contract() {
    let contact = AbsoluteContact::from_normalized(0.5, 0.25, 0.5);
    for action in 0..=u8::MAX {
        log_missing_pen(&contact, (0.0, 0.0), false, action);
        note_pen_action(action);
    }
    let log = crate::test_logging::text();
    assert!(log.contains("Mode is now second screen"));
    assert!(log.contains("Mode is now pen-only"));
    assert!(log.contains("Pen DOWN"));
    assert!(log.contains("Pen UP"));
}
