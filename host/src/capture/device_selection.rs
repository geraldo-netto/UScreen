//! Resolve portable GPU requests into native handles before session construction.
use super::CaptureConfig;
use blent_config::{
    gpu::Policy,
    linux::gpu::{self, Device},
};

pub(crate) async fn select_gpu(config: &mut CaptureConfig, request: &str) {
    if request.is_empty() {
        return;
    } // Preserve legacy explicit device paths.
    resolve(
        config,
        request,
        &gpu::discover(),
        !cfg!(feature = "inproc-encoder"),
    );
    #[cfg(not(feature = "inproc-encoder"))]
    validate_requested(config, |config| async move {
        super::probe::measure(&config).await.is_ok()
    })
    .await;
    tracing::info!(requested_gpu = request, ?config.gpu_policy, encoder = %config.encoder, device = %config.vaapi_device,
        "Encoding device policy resolved; active device evidence is reported separately");
}

pub(crate) fn initial_encoder<'a>(encoder: &'a str, request: &str) -> &'a str {
    if !request.is_empty() && cfg!(feature = "inproc-encoder") {
        Policy::Software.encoder(encoder)
    } else {
        encoder
    }
}

fn resolve(config: &mut CaptureConfig, request: &str, devices: &[Device], supported: bool) {
    let mut catalog = gpu::catalog(devices);
    catalog.supported = supported;
    config.gpu_policy = catalog.policy(request);
    if let Some(device) = devices
        .iter()
        .find(|device| device.adapter.id == request && device.adapter.accessible)
    {
        config.vaapi_device = device.node.to_string_lossy().into_owned();
    }
    config.encoder = config.gpu_policy.encoder(&config.encoder).into();
}

#[cfg(not(feature = "inproc-encoder"))]
async fn validate_requested<F: std::future::Future<Output = bool>>(
    config: &mut CaptureConfig,
    probe: impl FnOnce(CaptureConfig) -> F,
) {
    // Auto already probes all allowed candidates. Software needs no GPU test.
    if config.encoder == "auto" || Policy::Software.allows(&config.encoder) {
        return;
    }
    if !probe(config.clone()).await {
        tracing::warn!(encoder = %config.encoder, "Requested GPU/codec probe failed; using H.264 software fallback");
        config.encoder = "libx264".into();
    }
}

#[cfg(test)]
mod tests;
