use super::*;

/// Only routes created successfully by this owner may be removed on retirement.
/// Existing matching routes are usable but remain externally owned.
pub struct Routes {
    serial: String,
    expected: [(u16, u16); 2],
    owned: Vec<(u16, u16)>,
}
impl Routes {
    pub fn new(serial: &str, ports: (u16, u16)) -> Result<Self> {
        ensure!(valid_serial(serial), "Invalid USB serial");
        Self::for_transport(serial, ports)
    }

    /// Shared route ownership also applies to Linux network/mDNS transports.
    /// USB connection admission continues to use the stricter `new` boundary.
    pub fn for_transport(serial: &str, ports: (u16, u16)) -> Result<Self> {
        ensure!(
            !serial.is_empty() && serial.len() <= 1024 && !serial.chars().any(char::is_control),
            "Invalid ADB serial"
        );
        blent_config::slot_ports(ports.0, ports.1, 1)?;
        Ok(Self {
            serial: serial.into(),
            expected: [(8890, ports.0), (8891, ports.1)],
            owned: Vec::new(),
        })
    }
    /// Caller must prove both ADB aliases identify the same physical tablet.
    /// Owned reverse routes belong to that tablet, not its transient transport.
    pub(crate) fn retarget(&mut self, serial: &str) -> Result<()> {
        let checked = Self::for_transport(serial, (self.expected[0].1, self.expected[1].1))?;
        self.serial = checked.serial;
        Ok(())
    }
    pub fn serial(&self) -> &str {
        &self.serial
    }

    pub async fn prepare<C: Commands>(&mut self, adb: &Adb<C>) -> Result<()> {
        let listing = adb.routes(&self.serial).await?;
        let missing = blent_config::adb_reverse::missing(&listing, &self.expected)?;
        for (remote, local) in missing {
            adb.checked(
                args(
                    &self.serial,
                    &[
                        "reverse",
                        "--no-rebind",
                        &format!("tcp:{remote}"),
                        &format!("tcp:{local}"),
                    ],
                ),
                None,
            )
            .await?;
            if !self.owned.contains(&(remote, local)) {
                self.owned.push((remote, local));
            }
        }
        Ok(())
    }

    pub async fn retire<C: Commands>(&mut self, adb: &Adb<C>) -> Result<()> {
        if self.owned.is_empty() {
            return Ok(());
        }
        let listing = adb.routes(&self.serial).await?;
        let present = blent_config::adb_reverse::matching(&listing, &self.owned)?;
        self.owned.retain(|route| present.contains(route));
        while let Some(&(remote, _)) = self.owned.last() {
            adb.checked(
                args(
                    &self.serial,
                    &["reverse", "--remove", &format!("tcp:{remote}")],
                ),
                None,
            )
            .await?;
            self.owned.pop();
        }
        Ok(())
    }
}
