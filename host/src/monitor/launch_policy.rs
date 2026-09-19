//! One launch attempt per observed attachment, shared across proven route aliases.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

// Discovery is bounded, but a continuously changing inventory may postpone
// pruning. Exhaustion falls back to manual opening instead of growing forever.
const MAX_ROUTES: usize = 256;

struct Route {
    incarnation: Arc<()>,
    identity: Option<String>,
    launched: bool,
    active: bool,
}

#[derive(Default, Clone)]
pub(super) struct Launches(Arc<Mutex<HashMap<String, Route>>>);

pub(super) struct Ticket {
    owner: Launches,
    serial: String,
    incarnation: Arc<()>,
}

impl Launches {
    pub fn observe(&self, serial: &str, identity: Option<&str>) -> Option<Ticket> {
        if serial.is_empty() || serial.contains('\0') {
            return None;
        }
        let identity = identity.filter(|id| !id.is_empty());
        let mut routes = self.0.lock().unwrap();
        if !routes.contains_key(serial) && routes.len() >= MAX_ROUTES {
            return None;
        }
        let inherited = already_launched(&routes, identity, serial);
        let route = routes.entry(serial.into()).or_insert_with(|| Route {
            incarnation: Arc::new(()),
            identity: None,
            launched: false,
            active: true,
        });
        route.observe(identity, inherited);
        Some(Ticket {
            owner: self.clone(),
            serial: serial.into(),
            incarnation: route.incarnation.clone(),
        })
    }

    pub fn retain(&self, inventory: &[String]) {
        self.0
            .lock()
            .unwrap()
            .retain(|serial, _| inventory.contains(serial));
    }

    pub fn refresh(&self, inventory: &[String]) {
        for (serial, route) in self.0.lock().unwrap().iter_mut() {
            route.active &= inventory.contains(serial);
        }
    }
}

impl Route {
    fn observe(&mut self, identity: Option<&str>, inherited: bool) {
        if !self.active {
            self.incarnation = Arc::new(());
            self.launched = false;
            self.active = true;
        }
        if let Some(identity) = identity {
            if self.identity.as_deref().is_some_and(|old| old != identity) {
                self.incarnation = Arc::new(());
                self.launched = false;
            }
            self.identity = Some(identity.into());
        }
        self.launched |= inherited;
    }
}

impl Ticket {
    /// Consume synchronously after forwarding succeeds, before any Activity
    /// command can await. Cancellation cannot issue a second launch attempt.
    pub fn take(&self) -> bool {
        let mut routes = self.owner.0.lock().unwrap();
        let Some(route) = routes.get(&self.serial) else {
            return false;
        };
        if !route.active || !Arc::ptr_eq(&route.incarnation, &self.incarnation) || route.launched {
            return false;
        }
        let identity = route.identity.clone();
        for (serial, route) in routes.iter_mut() {
            if serial == &self.serial
                || identity
                    .as_ref()
                    .is_some_and(|id| route.identity.as_ref() == Some(id))
            {
                route.launched = true;
            }
        }
        true
    }
}

fn already_launched(routes: &HashMap<String, Route>, identity: Option<&str>, serial: &str) -> bool {
    identity.is_some_and(|id| {
        routes.iter().any(|(key, route)| {
            key != serial && route.identity.as_deref() == Some(id) && route.launched
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t445_aliases_replacement_and_late_jobs_have_distinct_launch_lifetimes() {
        let policy = Launches::default();
        let first = policy.observe("usb", None).unwrap();
        assert!(first.take());
        assert!(!first.take());
        policy.observe("usb", Some("tablet"));
        let network = policy.observe("network", Some("tablet")).unwrap();
        policy.retain(&["network".into()]);
        assert!(
            !network.take(),
            "T445: lost launch history after proven migration"
        );
        assert!(!first.take(), "T445: removed route consumed a launch");
        policy.retain(&[]);
        let fresh = policy.observe("network", Some("tablet")).unwrap();
        assert!(!network.take(), "T445: serial reuse revived a retired job");
        assert!(fresh.take());
        let replacement = policy.observe("network", Some("other-tablet")).unwrap();
        assert!(!fresh.take());
        assert!(replacement.take());
    }

    #[test]
    fn t445_pending_unknown_alias_observes_later_identity_without_double_launch() {
        let policy = Launches::default();
        let pending = policy.observe("network", None).unwrap();
        let usb = policy.observe("usb", Some("tablet")).unwrap();
        assert!(usb.take());
        policy.observe("network", Some("tablet"));
        assert!(!pending.take());
        let other = policy.observe("other", None).unwrap();
        assert!(other.take(), "T445: unknown peers must not share a launch");
        assert!(!policy.observe("other", Some("")).unwrap().take());
    }

    #[test]
    fn t445_registry_bounds_preserve_manual_fallback_and_existing_routes() {
        let policy = Launches::default();
        for invalid in ["", "\0", "a\0b"] {
            assert!(policy.observe(invalid, None).is_none());
        }
        for index in 0..MAX_ROUTES {
            assert!(policy.observe(&index.to_string(), None).is_some());
        }
        assert!(policy.observe("overflow", None).is_none());
        assert!(policy.observe("0", None).unwrap().take());
        policy.retain(&[]);
        for index in 0..=255u8 {
            let serial = format!("tablet-{}-{}", index, char::from(index));
            if index == 0 {
                assert!(policy.observe(&serial, None).is_none());
            } else {
                assert!(policy.observe(&serial, None).unwrap().take());
            }
        }
    }

    #[test]
    fn t445_missing_route_cancels_launch_before_pending_probes_finish() {
        let policy = Launches::default();
        let old = policy.observe("usb", Some("tablet")).unwrap();
        policy.refresh(&["unprobed-network".into()]);
        assert!(
            !old.take(),
            "T445: removed route launched while identity probe was pending"
        );
        let fresh = policy.observe("usb", Some("tablet")).unwrap();
        assert!(!old.take());
        assert!(fresh.take());
    }
}
