use super::*;

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
