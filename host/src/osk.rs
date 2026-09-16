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

use std::path::PathBuf;
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
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    (!digits.is_empty()).then_some(digits)
}

async fn set_mode(mode: &str) -> bool {
    // `mode` is a D-Bus int32; busctl needs to be told so.
    crate::kwin::set_property(OBJECT, IFACE, "mode", "i", mode).await
}

/// Turn the on-screen keyboard off, remembering how it was set.
async fn disable() {
    let path = state_path();
    // A state file already present means a previous run never restored. Keep
    // that value: it is the user's, whereas the current one is ours.
    if !path.exists() {
        let Some(current) = get_mode().await else {
            warn!("KWin's virtual keyboard interface is unavailable — leaving it alone");
            return;
        };
        if current == MODE_MANUAL {
            return; // Already how we want it; nothing to remember or undo.
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(&path, &current).is_err() {
            warn!("Could not save the keyboard setting — leaving it alone");
            return; // Without a way back, do not touch the user's desktop.
        }
    }

    if set_mode(MODE_MANUAL).await {
        info!("On-screen keyboard suppressed while UScreen touch devices exist");
    }
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
    let Ok(saved) = std::fs::read_to_string(path) else {
        return;
    };
    let saved = saved.trim();
    if !saved.is_empty() && apply(saved.to_string()).await {
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
