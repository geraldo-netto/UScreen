//! T660: child-process HKCU isolation remains outside the mutated GUI adapter.
use windows_sys::Win32::System::Registry::*;

pub struct RegistryFixture;

impl RegistryFixture {
    pub fn redirect(path: &str) -> Self {
        let path: Vec<_> = path.encode_utf16().chain(Some(0)).collect();
        let mut key = std::ptr::null_mut();
        assert_eq!(
            unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    path.as_ptr(),
                    0,
                    std::ptr::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_ALL_ACCESS,
                    std::ptr::null(),
                    &mut key,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        let status = unsafe { RegOverridePredefKey(HKEY_CURRENT_USER, key) };
        assert_eq!(unsafe { RegCloseKey(key) }, 0);
        assert_eq!(status, 0);
        Self
    }
}

impl Drop for RegistryFixture {
    fn drop(&mut self) {
        assert_eq!(
            unsafe { RegOverridePredefKey(HKEY_CURRENT_USER, std::ptr::null_mut()) },
            0
        );
    }
}

fn delete_fixture(path: &str) {
    let path: Vec<_> = path.encode_utf16().chain(Some(0)).collect();
    let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, path.as_ptr()) };
    assert!([0, windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND].contains(&status));
}

#[test]
fn t660_override_child() {
    if std::env::var_os("BLENT_T660_CHILD").is_none() {
        return;
    }
    use blent_config::windows::autostart::Registration;
    let token = blent_config::credentials::random_token().unwrap();
    let outer = format!("Software\\BlentTests\\T660-outer-{token}");
    let inner = format!("Software\\BlentTests\\T660-inner-{token}");
    let root = tempfile::tempdir().unwrap();
    let program = root.path().join("blent.exe");
    std::fs::write(&program, "private fixture").unwrap();
    let registry = RegistryFixture::redirect(&outer);
    Registration::at(&inner)
        .set_enabled(true, Some(&program))
        .unwrap();
    drop(registry);
    let escaped = Registration::at(&inner).enabled().unwrap();
    let contained = Registration::at(&format!("{outer}\\{inner}"))
        .enabled()
        .unwrap();
    delete_fixture(&outer);
    delete_fixture(&inner);
    assert!(
        !escaped,
        "T660: registration escaped the process-local registry root"
    );
    assert!(
        contained,
        "T660: registration did not reach the isolated root"
    );
}

#[test]
fn t660_registry_override_contains_writes_and_restores_hkcu() {
    use blent_config::commands::SyncCommandExt;
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "platform::windows::registry_fixture::t660_override_child",
            "--nocapture",
        ])
        .env("BLENT_T660_CHILD", "1")
        .output_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"));
    assert!(
        result.status.success(),
        "T660: {}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
