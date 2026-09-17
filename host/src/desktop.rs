//! Desktop capabilities used by adapters and diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Desktop {
    KdeWayland,
    X11,
    Other,
}

impl Desktop {
    pub fn current() -> Self {
        Self::detect(
            &std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
            &std::env::var("XDG_SESSION_TYPE").unwrap_or_default(),
        )
    }

    pub fn detect(desktops: &str, session: &str) -> Self {
        if session.eq_ignore_ascii_case("x11") {
            Self::X11
        } else if session.eq_ignore_ascii_case("wayland")
            && desktops
                .split(':')
                .any(|desktop| desktop.eq_ignore_ascii_case("KDE"))
        {
            Self::KdeWayland
        } else {
            Self::Other
        }
    }
}
