use super::*;
use crate::attachment::Attachment;

/// A failed preparation remains owned until its partial routes are retired.
/// Invalidation precedes every awaited cleanup, so old clients cannot survive it.
pub struct Connection {
    pub attachment: Attachment,
    ports: (u16, u16),
    launch: bool,
    routes: Option<Routes>,
    ready: bool,
}
impl Connection {
    pub fn new(attachment: Attachment, ports: (u16, u16), launch: bool) -> Result<Self> {
        blent_config::slot_ports(ports.0, ports.1, 1)?;
        Ok(Self {
            attachment,
            ports,
            launch,
            routes: None,
            ready: false,
        })
    }
    pub fn selected(&self) -> Option<&str> {
        self.routes.as_ref().map(Routes::serial)
    }
    pub fn serial(&self) -> Option<&str> {
        self.ready
            .then(|| self.routes.as_ref().map(Routes::serial))
            .flatten()
    }
    pub async fn connect<C: Commands>(&mut self, adb: &Adb<C>, serial: &str) -> Result<()> {
        ensure!(
            self.routes.is_none(),
            "Previous attachment still needs cleanup"
        );
        let routes = Routes::new(serial, self.ports)?;
        self.attachment
            .begin_with_transport(Some(format!("usb:{serial}")), Some(Transport::Usb));
        self.routes = Some(routes);
        if let Err(error) = self.prepare(adb).await {
            let cleanup = self.disconnect(adb).await;
            return Err(error.context(format!("USB preparation failed; cleanup: {cleanup:?}")));
        }
        self.ready = true;
        let _ = self.attachment.send(true);
        Ok(())
    }
    async fn prepare<C: Commands>(&mut self, adb: &Adb<C>) -> Result<()> {
        let routes = self.routes.as_mut().context("No attachment selected")?;
        ensure!(
            adb.installed(routes.serial()).await == Some(true),
            "Matching Android app is unavailable"
        );
        routes.prepare(adb).await?;
        let token = self
            .attachment
            .token()?
            .context("USB preview requires authentication")?;
        adb.deliver(routes.serial(), &token, self.launch).await
    }
    pub async fn refresh<C: Commands>(&mut self, adb: &Adb<C>) -> Result<()> {
        ensure!(self.ready, "Attachment is not prepared");
        self.routes
            .as_mut()
            .context("No attachment selected")?
            .prepare(adb)
            .await
    }
    pub async fn redeliver<C: Commands>(&self, adb: &Adb<C>) -> Result<()> {
        let serial = self.serial().context("No prepared attachment")?;
        let token = self
            .attachment
            .token()?
            .context("Attachment credential unavailable")?;
        adb.deliver(serial, &token, false).await
    }
    pub async fn disconnect<C: Commands>(&mut self, adb: &Adb<C>) -> Result<()> {
        self.ready = false;
        let _ = self.attachment.send(false);
        if let Some(routes) = self.routes.as_mut() {
            routes.retire(adb).await?;
        }
        self.routes = None;
        Ok(())
    }

    /// Retire authentication immediately; route debt needs no live session.
    pub fn release_routes(&mut self) -> Option<Routes> {
        self.ready = false;
        let _ = self.attachment.send(false);
        self.routes.take()
    }
}
