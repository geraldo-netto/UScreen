//! T405: task ownership cancels this server; its old running flag never changed.
use super::*;
use crate::poll_probe::Probe;

#[tokio::test(start_paused = true)]
async fn t405_idle_input_listener_has_no_periodic_wakeups() {
    let (mode, _mode_rx) = watch::channel(false);
    let (_card, card_rx) = watch::channel(None);
    let (_tablet, tablet_rx) = watch::channel(false);
    let server = InputServer::new(
        InputConfig {
            port: 0,
            touch: false,
            pen: false,
            pointer: false,
            ..Default::default()
        },
        None,
        mode,
        Default::default(),
        Default::default(),
        card_rx,
        tablet_rx,
    );
    let listener = server.bind().await.unwrap();
    let mut future = Box::pin(server.run_with_listener(listener));
    let probe = Arc::new(Probe::default());
    assert!(probe.poll(future.as_mut()).is_pending());
    tokio::task::yield_now().await;
    assert!(probe.poll(future.as_mut()).is_pending());
    probe.take();
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    assert_eq!(probe.take(), 0, "T405: idle input listener woke on a timer");
}
