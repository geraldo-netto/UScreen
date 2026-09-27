use super::*;
use windows_sys::Win32::System::Registry::{RegDeleteTreeW, HKEY_CURRENT_USER};

struct Fixture {
    key: String,
    root: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self {
            key: format!(
                "Software\\BlentTests\\T532-{}",
                crate::credentials::random_token().unwrap()
            ),
            root: tempfile::tempdir().unwrap(),
        }
    }
    fn program(&self, directory: &str) -> PathBuf {
        let directory = self.root.path().join(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("blent.exe");
        std::fs::write(
            &path,
            b"registration fixture; native launch has a separate test",
        )
        .unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let path = super::super::native::wide(self.key.as_ref()).unwrap();
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, path.as_ptr()) };
    }
}

#[test]
fn t532_enable_disable_upgrade_and_stale_path_preserve_other_entries() {
    let fixture = Fixture::new();
    let registration = Registration::at(&fixture.key);
    assert!(!registration.enabled().unwrap());
    registration.set_enabled(false, None).unwrap();
    assert!(Key::open(&fixture.key, KEY_QUERY_VALUE).unwrap().is_none());
    assert!(registration.set_enabled(true, None).is_err());
    let old = fixture.program("version one café 東京");
    let new = fixture.program("version two café 東京");
    let key = Key::create(&fixture.key).unwrap();
    key.write("unrelated", &[65, 66]).unwrap();
    assert!(!registration.enabled().unwrap());
    for _ in 0..3 {
        registration.set_enabled(true, Some(&old)).unwrap();
    }
    assert!(registration.enabled().unwrap());
    assert_eq!(
        owned_program(&key.read(VALUE).unwrap().unwrap()),
        Some(old.clone())
    );
    std::fs::remove_file(&old).unwrap();
    assert!(!registration.enabled().unwrap());
    registration.set_enabled(true, Some(&new)).unwrap();
    assert_eq!(owned_program(&key.read(VALUE).unwrap().unwrap()), Some(new));
    assert!(registration.enabled().unwrap());
    registration.set_enabled(false, None).unwrap();
    registration.set_enabled(false, None).unwrap();
    assert!(!registration.enabled().unwrap());
    assert_eq!(key.read("unrelated").unwrap(), Some(vec![65, 66]));
}

#[test]
fn t532_foreign_values_and_invalid_paths_are_never_overwritten() {
    let fixture = Fixture::new();
    let registration = Registration::at(&fixture.key);
    let program = fixture.program("valid");
    let key = Key::create(&fixture.key).unwrap();
    let foreign: Vec<_> =
        r#""C:\Windows\System32\cmd.exe" /c echo foreign"#.encode_utf16().collect();
    key.write(VALUE, &foreign).unwrap();
    for on in [false, true] {
        assert!(registration.set_enabled(on, Some(&program)).is_err());
    }
    assert!(!registration.enabled().unwrap());
    assert_eq!(key.read(VALUE).unwrap(), Some(foreign));
    for invalid in [
        Path::new("relative/blent.exe"),
        Path::new(""),
        Path::new("C:\\missing-t532\\blent.exe"),
        fixture.root.path(),
    ] {
        assert!(command(invalid).is_err(), "T532: {invalid:?}");
    }
    assert!(Registration::at("bad\0key").enabled().is_err());
    assert!(Registration::at("bad\0key")
        .set_enabled(true, Some(&program))
        .is_err());
    let long = fixture.program(&"x".repeat(230));
    assert!(command(&long).is_err());
}

#[test]
fn t532_bounded_invalid_command_fuzz_rejects_ambiguous_ownership() {
    let fixture = Fixture::new();
    let program = fixture.program("Unicode café 東京 and spaces");
    let valid = command(&program).unwrap();
    assert_eq!(owned_program(&valid), Some(program));
    for length in 0..valid.len() {
        assert!(owned_program(&valid[..length]).is_none());
    }
    for invalid in [0, 34] {
        for index in 1..valid.len() - SUFFIX.len() {
            let mut changed = valid.clone();
            changed[index] = invalid;
            assert!(owned_program(&changed).is_none());
        }
    }
    for length in [261, 512, 2048, 4096] {
        assert!(owned_program(&vec![65; length]).is_none());
    }
}
