//! Explicit executable discovery; never add the current directory implicitly.
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

pub fn is_executable(path: &Path) -> bool {
    path.is_file()
        && path.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("com")
        })
}

fn candidates(path: PathBuf) -> Vec<PathBuf> {
    if path.extension().is_some() {
        return vec![path];
    }
    vec![path.with_extension("exe"), path.with_extension("com")]
}

pub fn find_in(name: &str, search_path: &OsStr) -> Option<PathBuf> {
    let path = Path::new(name);
    let roots: Vec<_> = if path.components().count() > 1 {
        vec![path.to_owned()]
    } else {
        std::env::split_paths(search_path)
            .filter(|root| root.is_absolute())
            .map(|root| root.join(name))
            .collect()
    };
    roots
        .into_iter()
        .flat_map(candidates)
        .find(|path| is_executable(path))
}

pub fn command_exists(name: &str) -> bool {
    find_in(name, &std::env::var_os("PATH").unwrap_or_default()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t493_executable_search_preserves_unicode_and_rejects_shell_files() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("tools café 東京");
        std::fs::create_dir(&directory).unwrap();
        let exe = directory.join("adb.exe");
        std::fs::write(&exe, "fixture").unwrap();
        let path = std::env::join_paths([directory.clone()]).unwrap();
        assert_eq!(find_in("adb", &path), Some(exe.clone()));
        assert_eq!(find_in(exe.to_str().unwrap(), "".as_ref()), Some(exe));
        let script = directory.join("unsafe.cmd");
        std::fs::write(&script, "fixture").unwrap();
        assert!(!is_executable(&script));
        assert_eq!(find_in("unsafe.cmd", &path), None);
        for invalid in ["", ".", "..", "missing", "missing.exe", "\0", "a\0b"] {
            assert_eq!(find_in(invalid, &path), None, "T493: {invalid:?}");
        }
        assert_eq!(find_in("adb", "relative;;".as_ref()), None);
        assert!(!is_executable(&directory));
        assert!(!command_exists("uscreen-t493-does-not-exist"));
    }
}
