//! Windows monitor/device factory. The portable worker owns all native lifetimes.
use super::*;
use blent_config::{
    input_mapping::MonitorInventory,
    storage::ConfigStore,
    windows::{direct_input::NativeInput, monitors::NativeInventory},
};

struct Native {
    store: ConfigStore,
}
impl Environment for Native {
    type Device = NativeInput;
    fn snapshot(&self) -> Result<Snapshot> {
        NativeInventory.snapshot()
    }
    fn create(&self, mode: Mode) -> Result<NativeInput> {
        NativeInput::new(mode)
    }
    fn save(&self, config: &Config) -> Result<()> {
        self.store.update(|current| {
            ensure!(
                current.direct_input.as_ref().map(|value| &value.monitor) == Some(&config.monitor),
                "Monitor preference changed; restart input"
            );
            current.direct_input = Some(config.clone());
            Ok(())
        })?;
        Ok(())
    }
}
pub fn native(mut config: Config, touch: bool, mouse: bool, store: ConfigStore) -> Result<Backend> {
    if let Some(saved) = store
        .load()
        .direct_input
        .filter(|saved| saved.monitor == config.monitor)
    {
        config.mode = saved.mode;
    }
    Backend::new(config, touch, mouse, move || Native { store })
}

#[cfg(test)]
#[path = "../../../../testdata/input_window.rs"]
mod window;

#[cfg(test)]
mod tests;
