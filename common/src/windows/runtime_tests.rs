//! T493: native runtime acceptance, confined to fresh temporary directories.
use super::*;
use crate::commands::SyncCommandExt;
use crate::windows::{native::Local, security::User};
use std::os::windows::{io::AsRawHandle, process::CommandExt};
use windows_sys::Win32::{
    Security::{Authorization::*, *},
    Storage::FileSystem::FILE_ALL_ACCESS,
};

fn assert_owner_only_file(path: &Path) {
    // T637: permanent native regression; do not accept a default group owner.
    let file = std::fs::File::open(path).unwrap();
    let user = User::current().unwrap();
    let mut owner = std::ptr::null_mut();
    let mut acl = std::ptr::null_mut();
    let mut descriptor = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                std::ptr::null_mut(),
                &mut acl,
                std::ptr::null_mut(),
                &mut descriptor,
            )
        },
        0
    );
    let _descriptor = Local(descriptor);
    assert!(user.matches(owner));
    assert!(!acl.is_null());
    assert_eq!(unsafe { (*acl).AceCount }, 1);
    let mut entry = std::ptr::null_mut();
    assert_ne!(unsafe { GetAce(acl, 0, &mut entry) }, 0);
    let ace = unsafe { &*entry.cast::<ACCESS_ALLOWED_ACE>() };
    assert_eq!(ace.Header.AceType, 0);
    assert_eq!(ace.Header.AceFlags & INHERIT_ONLY_ACE as u8, 0);
    assert_eq!(ace.Mask, FILE_ALL_ACCESS);
    assert!(user.matches((&ace.SidStart as *const u32).cast_mut().cast()));
}

#[test]
fn t493_runtime_location_uses_the_native_local_known_folder() {
    assert_eq!(
        runtime_dir().unwrap(),
        crate::windows::paths::local().unwrap().join("blent")
    );
}

#[test]
fn t493_tokens_are_fresh_atomic_and_private_in_the_owned_directory() {
    let root = tempfile::tempdir().unwrap();
    let directory = Directory::create(&root.path().join("runtime café 東京")).unwrap();
    let mut previous = String::new();
    for _ in 0..32 {
        let token = new_session_token(&directory).unwrap();
        assert_eq!(token.len(), 64);
        assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(token, previous);
        assert_eq!(
            std::fs::read_to_string(directory.path().join("token")).unwrap(),
            token
        );
        assert_owner_only_file(&directory.path().join("token"));
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        previous = token;
    }
}

#[test]
fn t493_token_publish_failure_preserves_existing_state_and_cleans_temporary_file() {
    let root = tempfile::tempdir().unwrap();
    let directory = Directory::create(&root.path().join("runtime")).unwrap();
    let target = directory.path().join("token");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("keep"), b"unchanged").unwrap();
    assert!(new_session_token(&directory).is_err());
    assert_eq!(std::fs::read(target.join("keep")).unwrap(), b"unchanged");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn t493_runtime_rejects_junctions_without_modifying_the_target() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target café 東京");
    let junction = root.path().join("junction");
    let directory = Directory::create(&target).unwrap();
    let token = new_session_token(&directory).unwrap();
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c"])
        .raw_arg("mklink /j \"%BLENT_TEST_LINK%\" \"%BLENT_TEST_TARGET%\"")
        .env("BLENT_TEST_LINK", &junction)
        .env("BLENT_TEST_TARGET", &target)
        .output_bounded()
        .unwrap();
    assert!(
        output.status.success(),
        "T493: junction fixture: {output:?}"
    );
    let error = Directory::create(&junction)
        .err()
        .expect("T493: accepted a junction");
    assert!(error.to_string().contains("reparse point"), "{error:#}");
    assert_eq!(
        std::fs::read_to_string(target.join("token")).unwrap(),
        token
    );
    // Remove the junction itself while the real target remains pinned.
    std::fs::remove_dir(junction).unwrap();
    assert_owner_only_file(&target.join("token"));
}

#[test]
fn t493_lease_retirement_preserves_a_replaced_owner_record() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("runtime");
    let lease = Lease::acquire(Directory::create(&path).unwrap()).unwrap();
    assert_owner_only_file(&path.join("daemon.json"));
    assert_owner_only_file(&path.join("daemon.lock"));
    std::fs::write(path.join("daemon.json"), b"new owner record").unwrap();
    drop(lease);
    assert_eq!(
        std::fs::read(path.join("daemon.json")).unwrap(),
        b"new owner record"
    );
    assert!(Lease::acquire(Directory::create(&path).unwrap()).is_ok());
}
