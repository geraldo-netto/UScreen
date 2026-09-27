use super::*;

thread_local! {
    static T663_REPLACEMENT: std::cell::RefCell<Option<Vec<u16>>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn replace_before_data_read(key: &Key) {
    if let Some(value) = T663_REPLACEMENT.with(|value| value.borrow_mut().take()) {
        key.write("t663", &value).unwrap();
    }
}

#[test]
fn t663_read_uses_actual_size_when_value_shrinks_after_probe() {
    let path = format!(
        "Software\\BlentTests\\T663-{}",
        crate::credentials::random_token().unwrap()
    );
    let key = Key::create(&path).unwrap();
    let mut observations = Vec::new();
    for length in [0, 1, 2, 255, 1023, 2046] {
        key.write("t663", &vec![65; 2047]).unwrap();
        let expected = vec![0x03bb; length];
        T663_REPLACEMENT.with(|value| *value.borrow_mut() = Some(expected.clone()));
        observations.push((expected, key.read("t663")));
    }
    drop(key);
    let path = super::super::native::wide(path.as_ref()).unwrap();
    assert_eq!(
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, path.as_ptr()) },
        0
    );
    for (expected, actual) in observations {
        assert_eq!(
            actual.unwrap(),
            Some(expected),
            "T663: shrink after size probe"
        );
    }
}

#[test]
fn t659_registry_drop_child() {
    if std::env::var_os("BLENT_T659_CHILD").is_none() {
        return;
    }
    let path = format!(
        "Software\\BlentTests\\T659-{}",
        crate::credentials::random_token().unwrap()
    );
    let key = Key::create(&path).unwrap();
    let handle = key.0;
    let name = super::super::native::wide("value".as_ref()).unwrap();
    key.write("value", &[65]).unwrap();
    let mut size = 0;
    assert_eq!(
        unsafe {
            RegQueryValueExW(
                handle,
                name.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        },
        0
    );
    drop(key);
    let status = unsafe {
        RegQueryValueExW(
            handle,
            name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    let path = super::super::native::wide(path.as_ref()).unwrap();
    assert_eq!(
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, path.as_ptr()) },
        0
    );
    assert_eq!(status, windows_sys::Win32::Foundation::ERROR_INVALID_HANDLE);
}

#[test]
fn t659_registry_drop_closes_owned_handle() {
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "windows::registry::tests::t659_registry_drop_child",
            "--nocapture",
        ])
        .env("BLENT_T659_CHILD", "1")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"));
    assert!(
        result.status.success(),
        "T659: {}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn t532_registry_rejects_permission_failures_and_malformed_values() {
    let path = format!(
        "Software\\BlentTests\\T532-registry-{}",
        crate::credentials::random_token().unwrap()
    );
    let key = Key::create(&path).unwrap();
    let name = super::super::native::wide("test".as_ref()).unwrap();
    key.write("test", &[65, 66]).unwrap();
    let reader = Key::open(&path, KEY_QUERY_VALUE).unwrap().unwrap();
    assert_eq!(reader.read("test").unwrap(), Some(vec![65, 66]));
    assert!(reader.write("test", &[67]).is_err());
    assert!(reader.remove("test").is_err());
    assert!(Key::open(&path, 0xffff_ffff).is_err());
    assert!(Key::create("bad\0key").is_err());
    for value in [vec![0], vec![65; 2048]] {
        assert!(key.write("test", &value).is_err());
    }
    assert!(key.read("bad\0name").is_err());
    assert!(key.write("bad\0name", &[65]).is_err());
    assert!(key.remove("bad\0name").is_err());
    for (kind, bytes) in [
        (REG_DWORD, vec![0u8; 4]),
        (REG_SZ, vec![]),
        (REG_SZ, vec![65]),
        (REG_SZ, vec![65, 0]),
        (REG_SZ, vec![65, 0, 0, 0, 66, 0, 0, 0]),
        (REG_SZ, vec![0; 4098]),
    ] {
        // RegSetValueExW can include a following NUL when given an unterminated
        // string. Keep initialized nonzero padding so this fixture consistently
        // stores malformed bytes instead of depending on adjacent allocator data.
        let mut guarded = bytes.clone();
        guarded.extend([0x55; 8]);
        assert_eq!(
            unsafe {
                RegSetValueExW(
                    key.0,
                    name.as_ptr(),
                    0,
                    kind,
                    guarded.as_ptr(),
                    bytes.len() as u32,
                )
            },
            0
        );
        let result = key.read("test");
        assert!(result.is_err(), "T532 accepted malformed registry value: kind={kind}, bytes={bytes:?}, result={result:?}");
    }
    key.remove("test").unwrap();
    key.remove("test").unwrap();
    assert_eq!(key.read("test").unwrap(), None);
    drop((reader, key));
    let path = super::super::native::wide(path.as_ref()).unwrap();
    assert_eq!(
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, path.as_ptr()) },
        0
    );
}
