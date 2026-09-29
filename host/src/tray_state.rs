//! Copyright (c) 2026 Geraldo Netto.
//! Shared tray presentation and action policy. Native adapters own shell resources.
use anyhow::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Starting,
    Unavailable,
    Waiting,
    Prepared(u8),
    Network(u8),
    Screen,
    Pen,
    Stopping,
}
impl State {
    pub fn usb(assignments: usize) -> Self {
        match assignments {
            0 => Self::Waiting,
            1..=4 => Self::Prepared(assignments as u8),
            _ => Self::Unavailable,
        }
    }
    pub fn connections(sessions: &[blent_config::tablets::TabletSession]) -> Self {
        match Self::usb(sessions.len()) {
            Self::Prepared(count)
                if sessions.iter().any(|s| {
                    blent_config::adb::transport_of(&s.serial)
                        == blent_config::adb::Transport::Network
                }) =>
            {
                Self::Network(count)
            }
            state => state,
        }
    }
    pub fn streaming(present: bool, pen: bool) -> Self {
        match (present, pen) {
            (false, _) => Self::Waiting,
            (true, true) => Self::Pen,
            (true, false) => Self::Screen,
        }
    }
    pub fn line(self) -> String {
        match self {
            Self::Starting => "Starting Blent".into(),
            Self::Unavailable => "USB connection unavailable".into(),
            Self::Waiting => "No tablet connected".into(),
            Self::Prepared(count) => format!("USB prepared: {count} tablet(s)"),
            Self::Network(count) => format!("Network ADB prepared: {count} tablet(s)"),
            Self::Screen => "Second screen".into(),
            Self::Pen => "Graphics tablet — the pen drives this screen".into(),
            Self::Stopping => "Stopping Blent".into(),
        }
    }
    pub fn preview_tooltip(self) -> String {
        format!(
            "Blent: {}\nDisplay/stylus unavailable; touch/mouse preview opt-in",
            self.line()
        )
    }
    pub fn dispatch(self, id: usize, actions: &impl Actions) -> Result<()> {
        if self == Self::Stopping {
            return Ok(());
        }
        match id {
            SETTINGS => actions.settings(),
            QUIT => {
                actions.quit();
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
pub const SETTINGS: usize = 1;
pub const QUIT: usize = 2;
pub const RELEASE: usize = 3;

pub fn release_label(release: Option<&str>) -> Option<String> {
    let release = release.filter(|value| value.len() <= 128)?;
    blent_config::release::newer_tag(release, crate::update::current_version())
        .map(|tag| format!("Update available: {tag}"))
}
pub fn tooltip(state: State, release: Option<&str>) -> String {
    let mut text = state.preview_tooltip();
    if let Some(label) = release_label(release) {
        text.push_str(&format!("\n{label}"));
    }
    text
}
pub fn dispatch(
    state: State,
    release: Option<&str>,
    id: usize,
    actions: &impl Actions,
) -> Result<()> {
    if id == RELEASE && state != State::Stopping && release_label(release).is_some() {
        actions.release()
    } else {
        state.dispatch(id, actions)
    }
}

pub trait Actions {
    fn settings(&self) -> Result<()>;
    fn quit(&self);
    fn release(&self) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    struct Recorder {
        settings: Cell<usize>,
        quits: Cell<usize>,
        fail: bool,
        releases: Cell<usize>,
    }
    impl Actions for Recorder {
        fn settings(&self) -> Result<()> {
            self.settings.set(self.settings.get() + 1);
            anyhow::ensure!(!self.fail, "T531 launch failed");
            Ok(())
        }
        fn release(&self) -> Result<()> {
            self.releases.set(self.releases.get() + 1);
            anyhow::ensure!(!self.fail, "T696 launch failed");
            Ok(())
        }
        fn quit(&self) {
            self.quits.set(self.quits.get() + 1);
        }
    }
    #[test]
    fn t531_status_preserves_backend_truth_and_invalid_bounds() {
        for count in 0..=4 {
            assert_ne!(State::usb(count), State::Unavailable);
        }
        for count in [5, 255, 65536, usize::MAX] {
            assert_eq!(State::usb(count), State::Unavailable);
        }
        for state in [
            State::Starting,
            State::Unavailable,
            State::Waiting,
            State::Prepared(4),
            State::Network(4),
            State::Screen,
            State::Pen,
            State::Stopping,
        ] {
            assert!(!state.line().is_empty());
            assert!(state
                .preview_tooltip()
                .contains("Display/stylus unavailable; touch/mouse preview opt-in"));
        }
        assert_eq!(State::usb(2).line(), "USB prepared: 2 tablet(s)");
        assert_eq!(State::streaming(false, false), State::Waiting);
        assert_eq!(State::streaming(false, true), State::Waiting);
        assert_eq!(State::streaming(true, false), State::Screen);
        assert_eq!(State::streaming(true, true), State::Pen);
    }
    #[test]
    fn t531_actions_reject_unknown_ids_and_stale_shutdown_commands() {
        let actions = Recorder {
            settings: Cell::new(0),
            quits: Cell::new(0),
            fail: false,
            releases: Cell::new(0),
        };
        for id in (0..=256).chain([usize::MAX, 65537, 65538]) {
            State::Waiting.dispatch(id, &actions).unwrap();
            State::Stopping.dispatch(id, &actions).unwrap();
        }
        assert_eq!((actions.settings.get(), actions.quits.get()), (1, 1));
        let failing = Recorder {
            fail: true,
            ..actions
        };
        assert!(State::Waiting.dispatch(SETTINGS, &failing).is_err());
    }
    #[test]
    fn t696_release_action_requires_valid_current_notification() {
        let actions = Recorder {
            settings: Cell::new(0),
            quits: Cell::new(0),
            releases: Cell::new(0),
            fail: false,
        };
        for release in [None, Some("invalid"), Some("1.0.0"), Some("999.0.0")] {
            for state in [State::Waiting, State::Stopping] {
                dispatch(state, release, RELEASE, &actions).unwrap();
                assert_eq!(
                    tooltip(state, release).contains("Update available"),
                    release == Some("999.0.0")
                );
            }
        }
        assert_eq!(actions.releases.get(), 1);
        assert!(release_label(Some(&"9".repeat(129))).is_none());
        let failed = Recorder {
            fail: true,
            ..actions
        };
        assert!(dispatch(State::Waiting, Some("999.0.0"), RELEASE, &failed).is_err());
    }
    #[tokio::test]
    async fn t531_watch_coalesces_stale_snapshots_and_reports_producer_exit() {
        let (producer, mut receiver) = tokio::sync::watch::channel(State::Starting);
        producer.send_replace(State::Prepared(1));
        producer.send_replace(State::Waiting);
        receiver.changed().await.unwrap();
        assert_eq!(*receiver.borrow_and_update(), State::Waiting);
        drop(producer);
        assert!(receiver.changed().await.is_err());
    }
}
