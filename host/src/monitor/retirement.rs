//! Cancel producers and retire device routes before a port slot can be reused.
use super::*;

impl Monitor {
    pub(super) fn retire(
        &mut self,
        serial: String,
        instance: u32,
        session: Option<ExtraSession>,
        task: Option<tokio::task::JoinHandle<Mutation>>,
    ) {
        if let Some(session) = &session {
            let _ = session.tablet_tx.send(false);
        }
        self.retiring_slots.insert(instance);
        self.retiring_routes.insert(serial.clone(), instance);
        let adb = self.config.adb.clone();
        let routes = self.routes.remove(&serial);
        self.retiring.spawn(async move {
            if let Some(task) = task {
                let _ = task.await;
            }
            if let Some(session) = session {
                session.stop().await;
            }
            if let Some(routes) = routes {
                if let Err(error) = routes.retire(&adb).await {
                    // Offline cleanup remains bounded; credentials were revoked
                    // before retirement, and foreign routes are never removed.
                    warn!("Could not retire owned routes on {serial}: {error}");
                }
            }
            (serial, instance)
        });
    }

    pub(super) fn retired(&mut self, (serial, instance): (String, u32)) {
        self.retiring_routes.remove(&serial);
        if !self.retiring_routes.values().any(|slot| *slot == instance) {
            self.retiring_slots.remove(&instance);
        }
    }

    pub(super) fn remove_assignment(&mut self, serial: &str) {
        let ready = self.ready.remove(serial);
        self.attachments.remove(serial);
        let task = self.mutations.retire(serial);
        self.token_retries.remove(serial);
        let pending = self.pending.remove(serial);
        let assigned = ready || pending.is_some() || task.is_some();
        let session = pending
            .and_then(|pending| pending.session)
            .or_else(|| self.extras.remove(serial));
        let instance = session.as_ref().map_or(0, |session| session.instance);
        if (assigned || session.is_some()) && !self.retiring_routes.contains_key(serial) {
            self.retire(serial.into(), instance, session, task);
        }
    }
}
