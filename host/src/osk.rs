//! Suppressing the desktop's on-screen keyboard while UScreen touch devices exist.
//!
//! The tablet's touch device is a genuine touchscreen as far as the desktop is
//! concerned, so KDE offers the virtual keyboard whenever a text field takes
//! focus. On a screen being used as a monitor — or one you are only drawing on
//! — that is never wanted.
//!
//! Done over KWin's D-Bus interface rather than by writing kwinrc. Writing the
//! config file looks like the obvious route and does change the value on disk,
//! but KWin does not re-read it: verified by setting `VirtualKeyboardMode=0` in
//! the file and finding the live property still reporting 1, with the keyboard
//! duly appearing. Setting the property takes effect immediately, and has the
//! further advantage of leaving the user's saved configuration untouched.
//!
//! The previous value still goes to disk, so a daemon that was killed rather
//! than stopped can be undone by the next run.

use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

const OBJECT: &str = "/VirtualKeyboard";
const IFACE: &str = "org.kde.kwin.VirtualKeyboard";
/// Only when explicitly asked for, rather than on touch input.
const MODE_MANUAL: &str = "0";

static KEYBOARD_OPERATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn state_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".local/share/uscreen/osk-restore")
}

async fn get_mode() -> Option<String> {
    let raw = crate::kwin::get_property(OBJECT, IFACE, "mode").await?;
    valid_mode(&raw).map(str::to_owned)
}

async fn set_mode(mode: &str) -> bool {
    // `mode` is a D-Bus int32; busctl needs to be told so.
    crate::kwin::set_property(OBJECT, IFACE, "mode", "i", mode).await
}

/// Turn the on-screen keyboard off, remembering how it was set.
async fn disable() {
    disable_from(&state_path(), get_mode, || set_mode(MODE_MANUAL)).await;
}

async fn disable_from<Get, GetFuture, Apply, ApplyFuture>(
    path: &std::path::Path,
    get: Get,
    apply: Apply,
) where
    Get: FnOnce() -> GetFuture,
    GetFuture: std::future::Future<Output = Option<String>>,
    Apply: FnOnce() -> ApplyFuture,
    ApplyFuture: std::future::Future<Output = bool>,
{
    if !backup_ready(path, get).await {
        return;
    }
    if apply().await {
        info!("On-screen keyboard suppressed while UScreen touch devices exist");
    }
}

// KWin InputMethod::VirtualKeyboardVisibility: Never, NonMouseInput, AnyInput.
// https://github.com/KDE/kwin/blob/master/src/inputmethod.h
fn valid_mode(raw: &str) -> Option<&str> {
    match raw.trim() {
        mode @ ("0" | "1" | "2") => Some(mode),
        _ => None,
    }
}

fn invalid_backup() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid keyboard restore state")
}

fn sync_backup_directory(path: &Path) -> io::Result<()> {
    std::fs::File::open(path.parent().ok_or_else(invalid_backup)?)?.sync_all()
}

fn read_backup(path: &Path) -> io::Result<Option<String>> {
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !file.metadata()?.is_file() {
        return Err(invalid_backup());
    }
    let mut raw = String::new();
    file.read_to_string(&mut raw)?;
    let mode = valid_mode(&raw).ok_or_else(invalid_backup)?.to_owned();
    file.sync_all()?;
    sync_backup_directory(path)?;
    Ok(Some(mode))
}

fn save_backup_with(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File) -> io::Result<()>,
) -> io::Result<()> {
    let parent = path.parent().ok_or_else(invalid_backup)?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    write(temporary.as_file_mut())?;
    temporary.as_file().sync_all()?;
    // Atomic publication never truncates an older restore value, even if it
    // appeared after our initial read. A failed write leaves no empty backup.
    temporary
        .persist_noclobber(path)
        .map_err(|error| error.error)?;
    sync_backup_directory(path)
}

async fn backup_ready<Get, GetFuture>(path: &Path, get: Get) -> bool
where
    Get: FnOnce() -> GetFuture,
    GetFuture: std::future::Future<Output = Option<String>>,
{
    match read_backup(path) {
        Ok(Some(_)) => return true,
        Ok(None) => {}
        Err(error) => {
            warn!("Cannot use keyboard restore state at {:?}: {} — leaving keyboard unchanged; repair the saved state before retrying", path, error);
            return false;
        }
    }
    let current = get().await;
    let Some(mode) = current.as_deref().and_then(valid_mode) else {
        warn!("KWin's virtual keyboard mode is unavailable or unsupported — leaving it alone");
        return false;
    };
    if mode == MODE_MANUAL {
        return false;
    }
    if let Err(error) = save_backup_with(path, |file| file.write_all(mode.as_bytes())) {
        warn!(
            "Could not durably save keyboard state: {} — leaving it alone",
            error
        );
        return false;
    }
    true
}

/// Put the on-screen keyboard back the way the user had it.
pub async fn restore() {
    let _operation = KEYBOARD_OPERATION.lock().await;
    restore_unlocked().await;
}

async fn restore_unlocked() {
    restore_from(&state_path(), |saved| async move { set_mode(&saved).await }).await;
}

async fn restore_from<F, Fut>(path: &std::path::Path, apply: F)
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let saved = match read_backup(path) {
        Ok(Some(saved)) => saved,
        Ok(None) => return,
        Err(error) => {
            warn!(
                "Keyboard restore state is unusable: {} — retained for repair",
                error
            );
            return;
        }
    };
    if apply(saved).await {
        info!("On-screen keyboard setting restored");
        let _ = std::fs::remove_file(path);
    } else {
        warn!("Keyboard restoration failed — saved setting retained for retry");
    }
}

/// Device counts can change while D-Bus is in flight. Serialize with shutdown
/// restoration and reconcile again before releasing ownership of the keyboard.
pub async fn sync_touch_state(devices: &std::sync::atomic::AtomicUsize) {
    reconcile(devices, &KEYBOARD_OPERATION, |wanted| async move {
        if wanted {
            disable().await;
        } else {
            restore_unlocked().await;
        }
    })
    .await;
}

async fn reconcile<F, Fut>(
    devices: &std::sync::atomic::AtomicUsize,
    serial: &tokio::sync::Mutex<()>,
    mut apply: F,
) where
    F: FnMut(bool) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let _operation = serial.lock().await;
    loop {
        let wanted = devices.load(std::sync::atomic::Ordering::SeqCst) > 0;
        apply(wanted).await;
        if wanted == (devices.load(std::sync::atomic::Ordering::SeqCst) > 0) {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[tokio::test]
    async fn t334_invalid_existing_backup_never_suppresses_or_restores() {
        for contents in ["", "garbage", "mode2", "-1", "3", "2147483648"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("osk-restore");
            std::fs::write(&path, contents).unwrap();
            let suppressed = AtomicBool::new(false);
            disable_from(
                &path,
                || async { Some("2".into()) },
                || async {
                    suppressed.store(true, Ordering::SeqCst);
                    true
                },
            )
            .await;
            assert!(
                !suppressed.load(Ordering::SeqCst),
                "T334: suppressed with invalid backup {contents:?}"
            );
            restore_from(&path, |_| async {
                panic!("T334: invalid saved mode reached D-Bus")
            })
            .await;
            assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
        }
    }

    #[tokio::test]
    async fn t334_unreadable_state_never_changes_the_keyboard() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("osk-restore");
        std::fs::create_dir(&path).unwrap();
        let suppressed = AtomicBool::new(false);
        disable_from(
            &path,
            || async { Some("2".into()) },
            || async {
                suppressed.store(true, Ordering::SeqCst);
                true
            },
        )
        .await;
        assert!(
            !suppressed.load(Ordering::SeqCst),
            "T334: unreadable backup allowed suppression"
        );
        restore_from(&path, |_| async { panic!("T334: unreadable mode applied") }).await;
        assert!(path.is_dir());
    }

    #[tokio::test]
    async fn t334_valid_backup_is_preserved_and_restoration_can_retry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("osk-restore");
        std::fs::write(&path, "2\n").unwrap();
        let mode = AtomicUsize::new(1);
        disable_from(
            &path,
            || async { panic!("T334: keep the previous user's value") },
            || async {
                mode.store(0, Ordering::SeqCst);
                true
            },
        )
        .await;
        assert_eq!(mode.load(Ordering::SeqCst), 0);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "2\n");
        restore_from(&path, |_| async { false }).await;
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "2\n");
        restore_from(&path, |saved| {
            mode.store(saved.parse().unwrap(), Ordering::SeqCst);
            async { true }
        })
        .await;
        assert_eq!(mode.load(Ordering::SeqCst), 2);
        assert!(!path.exists());
    }

    #[test]
    fn t334_failed_or_interrupted_writes_never_publish_empty_or_replace_valid_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("osk-restore");
        let failed = save_backup_with(&path, |file| {
            assert!(!path.exists(), "T334: incomplete state already published");
            file.write_all(b"2")?;
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "simulated interruption",
            ))
        });
        assert!(failed.is_err());
        assert!(
            !path.exists(),
            "T334: failed write published a restore file"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        save_backup_with(&path, |file| file.write_all(b"1")).unwrap();
        let replaced = save_backup_with(&path, |file| file.write_all(b"2"));
        assert_eq!(replaced.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "1");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn t334_new_state_must_be_saved_before_suppression() {
        for current in ["1", "2", "0", "bad", "3"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("new-directory/osk-restore");
            let suppressed = AtomicBool::new(false);
            disable_from(
                &path,
                || async { Some(current.into()) },
                || async {
                    assert_eq!(read_backup(&path).unwrap().as_deref(), Some(current));
                    suppressed.store(true, Ordering::SeqCst);
                    true
                },
            )
            .await;
            let expected = matches!(current, "1" | "2");
            assert_eq!(
                suppressed.load(Ordering::SeqCst),
                expected,
                "T334: {current}"
            );
            assert_eq!(path.exists(), expected);
        }
    }

    #[tokio::test]
    async fn t334_unwritable_state_destination_does_not_suppress() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("not-a-directory");
        std::fs::write(&parent, "retained").unwrap();
        disable_from(
            &parent.join("osk-restore"),
            || async { Some("2".into()) },
            || async { panic!("T334: keyboard suppressed despite failed persistence") },
        )
        .await;
        assert_eq!(std::fs::read_to_string(parent).unwrap(), "retained");
    }

    #[tokio::test]
    async fn t334_saved_state_must_be_a_regular_file_without_symlinks() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, "2").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let fifo = dir.path().join("fifo");
        let cpath = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
        for path in [&link, &fifo] {
            disable_from(
                path,
                || async { Some("2".into()) },
                || async { panic!("T334: nonregular backup allowed suppression") },
            )
            .await;
            restore_from(path, |_| async {
                panic!("T334: nonregular backup restored")
            })
            .await;
        }
        assert_eq!(std::fs::read_to_string(target).unwrap(), "2");
    }

    #[tokio::test]
    async fn t148_detach_during_disable_reconciles_current_device_count() {
        let devices = AtomicUsize::new(1);
        let serial = tokio::sync::Mutex::new(());
        let disabled = AtomicBool::new(false);
        let started = tokio::sync::Notify::new();
        let release = tokio::sync::Notify::new();
        tokio::join!(
            reconcile(&devices, &serial, async |wanted| {
                if wanted {
                    started.notify_one();
                    release.notified().await;
                }
                disabled.store(wanted, Ordering::SeqCst);
            }),
            async {
                started.notified().await;
                devices.store(0, Ordering::SeqCst);
                release.notify_one();
            }
        );
        assert!(
            !disabled.load(Ordering::SeqCst),
            "T148 late suppression survived detach"
        );
    }

    #[tokio::test]
    async fn t148_concurrent_device_changes_serialize_keyboard_writes() {
        let devices = AtomicUsize::new(1);
        let serial = tokio::sync::Mutex::new(());
        let release = tokio::sync::Notify::new();
        let disabled = AtomicBool::new(false);
        let calls = std::sync::Mutex::new(Vec::new());
        let first = reconcile(&devices, &serial, async |wanted| {
            calls.lock().unwrap().push(wanted);
            if wanted {
                release.notified().await;
            }
            disabled.store(wanted, Ordering::SeqCst);
        });
        tokio::pin!(first);
        assert!(futures_util::poll!(&mut first).is_pending());
        devices.store(0, Ordering::SeqCst);
        let second = reconcile(&devices, &serial, async |wanted| {
            calls.lock().unwrap().push(wanted);
            disabled.store(wanted, Ordering::SeqCst);
        });
        tokio::pin!(second);
        assert!(
            futures_util::poll!(&mut second).is_pending(),
            "T148 overlapping keyboard writes"
        );
        // Another tablet arrives before either operation finishes.
        devices.store(1, Ordering::SeqCst);
        release.notify_one();
        tokio::join!(first, second);
        assert!(disabled.load(Ordering::SeqCst));
        assert_eq!(*calls.lock().unwrap(), [true, true]);
        devices.store(0, Ordering::SeqCst);
        reconcile(&devices, &serial, async |wanted| {
            disabled.store(wanted, Ordering::SeqCst);
        })
        .await;
        assert!(!disabled.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn t111_restore_retries_preserved_setting_until_confirmed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("osk-restore");
        std::fs::write(&path, "2").unwrap();
        restore_from(&path, |value| async move {
            assert_eq!(value, "2");
            false
        })
        .await;
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "2");
        restore_from(&path, |value| async move {
            assert_eq!(value, "2");
            true
        })
        .await;
        assert!(!path.exists());
        restore_from(&path, |_| async { panic!("already restored") }).await;
    }
}
