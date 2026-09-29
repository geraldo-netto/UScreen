//! Preview composition: unavailable capture and optional non-stylus native input.
use crate::{
    input::{
        backend::{InputBackend, InputSink, PenSample},
        InputConfig,
    },
    media::EncoderSettings,
    session::*,
};
use std::{future::Future, pin::Pin, sync::Arc};
use tokio::sync::watch;

pub(super) fn prepare(
    config: &blent_config::FileConfig,
    instance: u32,
    ports: (u16, u16),
    store: &blent_config::storage::ConfigStore,
) -> anyhow::Result<Prepared> {
    let settings = EncoderSettings {
        encoder: "libx264".into(),
        fps: config.fps,
        bitrate: config.bitrate,
        width: config.width,
        height: config.height,
        quality: config.quality,
        width_mm: 310,
        height_mm: 194,
        stream_scale: config.stream_scale,
        geometry_ready: false,
        decoders: None,
        decoder_epoch: 0,
        selection: None,
    };
    let mut prepared = Spec {
        settings,
        instance,
        ports,
        token: Some(blent_config::credentials::random_token()?),
        devices: (false, false, false),
    }
    .prepare(
        watch::channel(true).0,
        Box::new(Unavailable),
        input_backend(config, store)?,
    );
    prepared.input = prepared.input.with_fixed_mode(true);
    Ok(prepared)
}

fn input_backend(
    config: &blent_config::FileConfig,
    store: &blent_config::storage::ConfigStore,
) -> anyhow::Result<Arc<dyn InputBackend>> {
    #[cfg(windows)]
    if let Some(direct) = &config.direct_input {
        return Ok(Arc::new(crate::input::direct::native(
            direct.clone(),
            config.input_touch,
            config.input_mouse,
            store.clone(),
        )?));
    }
    let _ = (config, store);
    Ok(Arc::new(Unavailable))
}
struct Unavailable;
impl CaptureBackend for Unavailable {
    fn resources(&self) -> CaptureResources {
        CaptureResources::default()
    }
    fn start(self: Box<Self>, mut context: CaptureContext) -> CaptureWorkers {
        CaptureWorkers {
            capture: tokio::spawn(async move {
                let _ = context.stop.wait_for(|stop| *stop).await;
            }),
            auxiliary: vec![],
        }
    }
}
impl InputSink for Unavailable {
    fn release_all(&self) {}
    fn touch(&self, _: (f64, f64, f64), _: u8, _: u8) {}
    fn pen(&self, _: PenSample, _: bool) {}
}
impl InputBackend for Unavailable {
    fn sink(&self) -> Arc<dyn InputSink> {
        Arc::new(Unavailable)
    }
    fn follow(
        &self,
        _: watch::Receiver<bool>,
        _: watch::Receiver<bool>,
        _: InputConfig,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(std::future::pending())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t525_unavailable_native_sink_safely_discards_invalid_events() {
        let sink = Unavailable.sink();
        for value in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -1.0,
            0.0,
            1.0,
            f64::MAX,
        ] {
            sink.touch((value, value, value), 255, 255);
            sink.pen(
                PenSample {
                    position: (value, value, value),
                    tilt: (value, value),
                    eraser: true,
                    action: 255,
                    button: Some(true),
                },
                true,
            );
            sink.release_all();
        }
    }
}

#[cfg(all(test, windows))]
#[test]
fn t673_preview_wires_explicit_direct_input_and_keeps_pen_unavailable() {
    let config = blent_config::FileConfig {
        direct_input: Some(blent_config::direct_input::Config {
            monitor: "test-monitor".into(),
            mode: blent_config::direct_input::Mode::DirectMouse,
        }),
        ..Default::default()
    };
    let root = tempfile::tempdir().unwrap();
    let store = blent_config::storage::ConfigStore::new(root.path().join("input.toml"));
    let backend = input_backend(&config, &store).unwrap();
    let status = backend.sink().direct_status().unwrap();
    assert_eq!(status.monitor, "test-monitor");
    assert!(status.touch && status.mouse);
    assert!(!status.negotiated);
}
