use super::*;
use tokio::io::{AsyncReadExt, DuplexStream};

fn pipe(capacity: usize) -> (FrameWriter<DuplexStream>, DuplexStream) {
    let (writer, reader) = tokio::io::duplex(capacity);
    (
        FrameWriter::new(writer, (2, 2), DEFAULT_WRITE_TIMEOUT).unwrap(),
        reader,
    )
}

#[tokio::test]
async fn t635_partial_writes_preserve_complete_frame_boundaries() {
    let (mut writer, mut reader) = pipe(2);
    let read = tokio::spawn(async move {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.unwrap();
        bytes
    });
    writer
        .write_frame((2, 2), &[1, 2, 3, 4, 5, 6])
        .await
        .unwrap();
    writer
        .write_frame((2, 2), &[7, 8, 9, 10, 11, 12])
        .await
        .unwrap();
    writer.shutdown().await.unwrap();
    writer.shutdown().await.unwrap();
    assert_eq!(read.await.unwrap(), (1..=12).collect::<Vec<_>>());
    assert_eq!(
        writer
            .write_frame((2, 2), &[0; 6])
            .await
            .unwrap_err()
            .kind(),
        io::ErrorKind::BrokenPipe
    );
}

#[tokio::test(start_paused = true)]
async fn t635_timeout_retires_partial_input_and_fresh_session_recovers() {
    let (mut writer, mut reader) = pipe(3);
    assert_eq!(
        writer
            .write_frame((2, 2), &[1, 2, 3, 4, 5, 6])
            .await
            .unwrap_err()
            .kind(),
        io::ErrorKind::TimedOut
    );
    assert_eq!(
        writer
            .write_frame((2, 2), &[7; 6])
            .await
            .unwrap_err()
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    let mut partial = Vec::new();
    reader.read_to_end(&mut partial).await.unwrap();
    assert_eq!(partial, [1, 2, 3]);
    let (mut fresh, mut reader) = pipe(6);
    fresh.write_frame((2, 2), &[7; 6]).await.unwrap();
    fresh.shutdown().await.unwrap();
    let mut recovered = Vec::new();
    reader.read_to_end(&mut recovered).await.unwrap();
    assert_eq!(recovered, [7; 6]);
}

#[tokio::test]
async fn t635_cancelling_a_pending_frame_closes_the_stream() {
    let (mut writer, mut reader) = pipe(2);
    let mut writing = Box::pin(writer.write_frame((2, 2), &[1, 2, 3, 4, 5, 6]));
    let mut prefix = [0; 2];
    tokio::select! {
        result = &mut writing => panic!("T635: write unexpectedly completed: {result:?}"),
        result = reader.read_exact(&mut prefix) => { result.unwrap(); }
    }
    drop(writing);
    assert_eq!(prefix, [1, 2]);
    assert_eq!(
        writer
            .write_frame((2, 2), &[7; 6])
            .await
            .unwrap_err()
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(reader.read(&mut prefix).await.unwrap(), 0);
}

#[tokio::test]
async fn t635_reader_exit_retires_the_writer() {
    let (mut writer, reader) = pipe(6);
    drop(reader);
    assert_eq!(
        writer
            .write_frame((2, 2), &[0; 6])
            .await
            .unwrap_err()
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        writer
            .write_frame((2, 2), &[0; 6])
            .await
            .unwrap_err()
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    writer.shutdown().await.unwrap();
}

#[tokio::test]
async fn t635_bounded_bad_lengths_and_resize_never_touch_the_stream() {
    let (mut writer, mut reader) = pipe(6);
    for length in (0..24).filter(|length| *length != 6) {
        assert_eq!(
            writer
                .write_frame((2, 2), &vec![255; length])
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
    for dimensions in [(0, 2), (2, 0), (1, 4), (4, 2), (2, 4), (u32::MAX, 2)] {
        assert_eq!(
            writer
                .write_frame(dimensions, &[255; 6])
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
    writer.write_frame((2, 2), &[1; 6]).await.unwrap();
    writer.shutdown().await.unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, [1; 6]);
}

#[test]
fn t635_geometry_and_write_budget_are_bounded() {
    for invalid in [0, 1, 3, 4097, u32::MAX] {
        assert!(FrameWriter::new(tokio::io::sink(), (invalid, 2), DEFAULT_WRITE_TIMEOUT).is_err());
        assert!(FrameWriter::new(tokio::io::sink(), (2, invalid), DEFAULT_WRITE_TIMEOUT).is_err());
    }
    for budget in [
        Duration::ZERO,
        MAX_WRITE_TIMEOUT + Duration::from_nanos(1),
        Duration::MAX,
    ] {
        assert!(FrameWriter::new(tokio::io::sink(), (2, 2), budget).is_err());
    }
    assert!(FrameWriter::new(tokio::io::sink(), (4096, 4096), MAX_WRITE_TIMEOUT).is_ok());
}

struct FaultyWriter(u8);
impl AsyncWrite for FaultyWriter {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        std::task::Poll::Ready(Ok(if self.0 == 0 { 0 } else { bytes.len() }))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        if self.0 == 2 {
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "closed encoder input",
            )))
        }
    }
}

#[tokio::test(start_paused = true)]
async fn t635_zero_writes_and_shutdown_failures_retire_input() {
    let mut zero = FrameWriter::new(FaultyWriter(0), (2, 2), DEFAULT_WRITE_TIMEOUT).unwrap();
    assert_eq!(
        zero.write_frame((2, 2), &[0; 6]).await.unwrap_err().kind(),
        io::ErrorKind::WriteZero
    );
    assert_eq!(
        zero.write_frame((2, 2), &[0; 6]).await.unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    for (mode, expected) in [(1, io::ErrorKind::BrokenPipe), (2, io::ErrorKind::TimedOut)] {
        let mut writer =
            FrameWriter::new(FaultyWriter(mode), (2, 2), DEFAULT_WRITE_TIMEOUT).unwrap();
        writer.write_frame((2, 2), &[0; 6]).await.unwrap();
        assert_eq!(writer.shutdown().await.unwrap_err().kind(), expected);
        assert_eq!(
            writer
                .write_frame((2, 2), &[0; 6])
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
        writer.shutdown().await.unwrap();
    }
}
