//! Attachment authentication never falls back to tokenless on entropy failure.
#[derive(Clone)]
pub(super) enum Authentication {
    Unmanaged,
    Disabled,
    Required(String),
    Unavailable,
}
impl Authentication {
    pub fn configured(token: Option<String>) -> Self {
        token.map_or(Self::Disabled, Self::Required)
    }
    pub fn rotate(&mut self, preserve: bool, generate: impl FnOnce() -> anyhow::Result<String>) {
        let needed =
            matches!(self, Self::Unavailable) || (!preserve && matches!(self, Self::Required(_)));
        if !needed {
            return;
        }
        *self = match generate() {
            Ok(token) => Self::Required(token),
            Err(error) => {
                tracing::error!("Attachment credential unavailable: {error}");
                Self::Unavailable
            }
        };
    }
    pub fn expected<'a>(&'a self, fallback: Option<&'a str>) -> anyhow::Result<Option<&'a str>> {
        match self {
            Self::Unmanaged => Ok(fallback),
            Self::Disabled => Ok(None),
            Self::Required(token) => Ok(Some(token)),
            Self::Unavailable => anyhow::bail!("attachment credential unavailable"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t444_rotation_failure_stays_closed_and_same_identity_can_retry() {
        let mut auth = Authentication::configured(Some("old".into()));
        auth.rotate(true, || panic!("same proven attachment rotated"));
        assert_eq!(auth.expected(None).unwrap(), Some("old"));
        auth.rotate(false, || anyhow::bail!("injected entropy failure"));
        assert!(auth.expected(None).is_err());
        assert!(auth
            .expected(Some("fallback must not bypass failure"))
            .is_err());
        auth.rotate(true, || Ok("new".into()));
        assert_eq!(auth.expected(None).unwrap(), Some("new"));
        for mut auth in [Authentication::Unmanaged, Authentication::configured(None)] {
            auth.rotate(false, || panic!("tokenless mode requested entropy"));
        }
        assert_eq!(
            Authentication::Unmanaged
                .expected(Some("standalone"))
                .unwrap(),
            Some("standalone")
        );
        assert_eq!(
            Authentication::Disabled.expected(Some("ignored")).unwrap(),
            None
        );
    }
}
