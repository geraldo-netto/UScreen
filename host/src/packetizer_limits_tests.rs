//! T407: reject oversized pre-publication assembly before queue admission.
use super::*;

fn nal(codec: Codec, kind: u8, payload: usize) -> Vec<u8> {
    let mut data = vec![0, 0, 0, 1];
    match codec {
        Codec::H264 => data.push(kind),
        Codec::Vp9 => unreachable!("Annex B fixture"),
        Codec::Hevc => data.extend([kind << 1, 1]),
    }
    data.push(0x80);
    data.resize(data.len() + payload, 0x55);
    data
}

async fn rejected(codec: Codec, data: &[u8]) {
    let (tx, _rx) = crate::video_queue::channel(8, Default::default());
    let result = read_loop(data, tx, Default::default(), Default::default(), codec).await;
    assert!(result.is_err(), "T407: oversized assembly was accepted");
}

#[tokio::test]
async fn t407_unfinished_nal_is_bounded_before_queue_admission() {
    for codec in [Codec::H264, Codec::Hevc] {
        rejected(codec, &nal(codec, 1, 9 * 1024 * 1024)).await;
    }
}

#[tokio::test]
async fn t407_prefix_only_access_unit_is_bounded() {
    for (codec, kind) in [(Codec::H264, 6), (Codec::Hevc, 39)] {
        let data = nal(codec, kind, 4096).repeat(2200);
        rejected(codec, &data).await;
    }
}

#[tokio::test]
async fn t407_combined_parameter_sets_are_bounded() {
    let mut data = Vec::new();
    for kind in [32, 33, 34] {
        data.extend(nal(Codec::Hevc, kind, 3 * 1024 * 1024));
    }
    rejected(Codec::Hevc, &data).await;
}

#[tokio::test]
async fn t407_idr_with_prepended_configuration_obeys_wire_limit() {
    let mut data = nal(Codec::H264, 7, 4 * 1024 * 1024);
    data.extend(nal(Codec::H264, 8, 16));
    data.extend(nal(Codec::H264, 5, 5 * 1024 * 1024));
    rejected(Codec::H264, &data).await;
}

#[test]
fn t407_stdout_read_avoids_scratch_to_parser_payload_copy() {
    let data = nal(Codec::H264, 1, 1024).repeat(50);
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (tx, _rx) = crate::video_queue::channel(8, Default::default());
    let (result, counts) = crate::allocation_probe::measure(|| {
        rt.block_on(read_loop(
            data.as_slice(),
            tx,
            Default::default(),
            Default::default(),
            Codec::H264,
        ))
    });
    result.unwrap();
    assert!(
        counts.explicit_copy_bytes <= data.len() as u64 + 2048,
        "T407: repeated input/assembly copies: {} for {} bytes",
        counts.explicit_copy_bytes,
        data.len()
    );
}

#[tokio::test]
async fn t407_cancellation_retires_queued_generation_with_partial_next_nal() {
    use tokio::io::AsyncWriteExt;
    let (mut writer, reader) = tokio::io::duplex(256);
    let (tx, mut receiver) = crate::video_queue::channel(8, Default::default());
    let task = tokio::spawn(read_loop(
        reader,
        tx,
        Default::default(),
        Default::default(),
        Codec::H264,
    ));
    writer
        .write_all(&[0, 0, 0, 1, 5, 0x80, 0x55, 0, 0, 0, 1, 1, 0x80])
        .await
        .unwrap();
    let packet = tokio::time::timeout(std::time::Duration::from_secs(2), receiver.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(packet.generation.load(std::sync::atomic::Ordering::Acquire));
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(!packet.generation.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(packet.data.as_ref(), &[0, 0, 0, 1, 5, 0x80, 0x55]);
}

#[tokio::test]
async fn t407_exact_frame_limit_survives_eof_and_read_boundaries() {
    let maximum = crate::video_queue::MAX_FRAME_BYTES;
    let data = nal(Codec::H264, 1, maximum - 6);
    assert_eq!(data.len(), maximum);
    let (tx, mut receiver) = crate::video_queue::channel(8, Default::default());
    read_loop(
        data.as_slice(),
        tx,
        Default::default(),
        Default::default(),
        Codec::H264,
    )
    .await
    .unwrap();
    assert_eq!(receiver.recv().await.unwrap().data.as_ref(), data);
    rejected(Codec::H264, &nal(Codec::H264, 1, maximum - 5)).await;
}
