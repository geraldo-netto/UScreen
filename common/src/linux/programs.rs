//! Executable discovery without an external `which` command.
use std::ffi::{CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

pub fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // Check effective credentials and ACLs as well as mode bits.
    unsafe { libc::faccessat(libc::AT_FDCWD, path.as_ptr(), libc::X_OK, libc::AT_EACCESS) == 0 }
}

pub fn find_in(name: &str, search_path: &OsStr) -> Option<PathBuf> {
    let explicit = Path::new(name);
    if explicit.components().count() > 1 {
        return is_executable(explicit).then(|| explicit.to_owned());
    }
    std::env::split_paths(search_path)
        .map(|directory| directory.join(name))
        .find(|path| is_executable(path))
}

pub fn command_exists(name: &str) -> bool {
    find_in(name, &std::env::var_os("PATH").unwrap_or_default()).is_some()
}
