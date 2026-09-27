// Copyright (c) 2026 Geraldo Netto
use super::*;
use std::{sync::Arc, time::Duration};

struct Fixture {
    frames: [outputs::Frames; 2],
    report: Report,
    worker: tokio::task::JoinHandle<()>,
}

impl Fixture {
    async fn start(lens: blent_config::camera::Lens) -> Self {
        let options = CameraOptions {
            profile: CameraProfile {
                lens,
                width: 160,
                height: 120,
                ..Default::default()
            },
            ..Default::default()
        };
        let frames = [watch::channel(None).0, watch::channel(None).0];
        let report = Report {
            state: watch::channel(State::Waiting).0,
            preview: watch::channel(None).0,
        };
        let owned = frames.clone();
        let status = report.clone();
        let worker = tokio::spawn(async move { frame_status(&owned, &options, &status).await });
        tokio::task::yield_now().await;
        Self {
            frames,
            report,
            worker,
        }
    }

    async fn publish(&self, lens: usize) {
        self.frames[lens].send_replace(Some(Arc::new(vec![128; 160 * 120 * 3 / 2])));
        tokio::task::yield_now().await;
    }

    async fn advance(&self, milliseconds: u64, expected: State) {
        tokio::time::advance(Duration::from_millis(milliseconds)).await;
        tokio::task::yield_now().await;
        assert_eq!(
            *self.report.state.borrow(),
            expected,
            "T704 freshness boundary"
        );
    }

    async fn stop(self) {
        self.worker.abort();
        assert!(self.worker.await.unwrap_err().is_cancelled());
        self.report.update(State::Stopped);
        tokio::time::advance(Duration::from_secs(3)).await;
        assert_eq!(*self.report.state.borrow(), State::Stopped);
        assert!(self.report.preview.borrow().is_none());
    }
}

#[tokio::test(start_paused = true)]
async fn t704_streaming_expires_at_two_seconds_and_recovers_on_selected_frames() {
    let fixture = Fixture::start(blent_config::camera::Lens::Front).await;
    fixture.publish(0).await;
    fixture.advance(1999, State::Streaming).await;
    fixture.publish(1).await; // Another lens cannot extend the selected stream's lifetime.
    fixture.advance(1, State::Waiting).await;
    assert!(fixture.report.preview.borrow().is_none());
    fixture.publish(0).await;
    fixture.advance(1999, State::Streaming).await;
    fixture.publish(0).await; // A fresh selected frame renews the full interval.
    fixture.advance(1, State::Streaming).await;
    fixture.advance(1999, State::Waiting).await;
    fixture.stop().await;
}

#[tokio::test(start_paused = true)]
async fn t704_stop_and_lens_replacement_cannot_inherit_a_previous_deadline() {
    let front = Fixture::start(blent_config::camera::Lens::Front).await;
    front.publish(0).await;
    front.stop().await;
    let rear = Fixture::start(blent_config::camera::Lens::Rear).await;
    rear.publish(0).await;
    rear.advance(2000, State::Waiting).await;
    rear.publish(1).await;
    rear.advance(1999, State::Streaming).await;
    rear.frames[1].send_replace(None);
    rear.advance(0, State::Waiting).await;
    rear.publish(1).await;
    rear.advance(2000, State::Waiting).await;
    rear.stop().await;
}
