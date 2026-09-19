//! One private diagnostic credential file per slot, atomically replaced.
use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;

pub(super) fn publish(directory: Option<&Path>, instance: u32, token: Option<&str>) -> Result<()> {
    let (Some(directory), Some(token)) = (directory, token) else {
        return Ok(());
    };
    anyhow::ensure!(
        token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid attachment credential"
    );
    let name = if instance == 0 {
        "token".into()
    } else {
        format!("token-{instance}")
    };
    let mut file =
        tempfile::NamedTempFile::new_in(directory).context("create private attachment token")?;
    file.write_all(token.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(directory.join(name))
        .context("publish attachment token")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn t444_slot_files_are_atomic_private_distinct_and_validate_input() {
        let root = tempfile::tempdir().unwrap();
        let old = "a".repeat(64);
        let new = "b".repeat(64);
        publish(Some(root.path()), 0, Some(&old)).unwrap();
        let previous = std::fs::File::open(root.path().join("token")).unwrap();
        publish(Some(root.path()), 0, Some(&new)).unwrap();
        use std::io::Read;
        let mut retained = String::new();
        (&previous).read_to_string(&mut retained).unwrap();
        assert_eq!(
            retained, old,
            "T444: publication modified an inode readers already opened"
        );
        for slot in [0, 1, 3, u32::MAX] {
            publish(Some(root.path()), slot, Some(&new)).unwrap();
            let name = if slot == 0 {
                "token".into()
            } else {
                format!("token-{slot}")
            };
            let path = root.path().join(name);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), new);
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        for size in [0, 1, 63, 65, 4096] {
            assert!(publish(Some(root.path()), 0, Some(&"a".repeat(size))).is_err());
        }
        for byte in 0u8..=255 {
            if byte.is_ascii_hexdigit() {
                continue;
            }
            let invalid = format!("{}{}", "a".repeat(63), char::from(byte));
            assert!(publish(Some(root.path()), 0, Some(&invalid)).is_err());
        }
        assert_eq!(
            std::fs::read_to_string(root.path().join("token")).unwrap(),
            new
        );
        publish(None, 0, Some(&old)).unwrap();
        publish(Some(root.path()), 0, None).unwrap();
        assert!(publish(Some(&root.path().join("missing")), 0, Some(&old)).is_err());
        std::fs::create_dir(root.path().join("token-2")).unwrap();
        assert!(publish(Some(root.path()), 2, Some(&old)).is_err());
        assert_eq!(
            std::fs::read_dir(root.path()).unwrap().count(),
            5,
            "failed writes left temporary credentials"
        );
    }
}
