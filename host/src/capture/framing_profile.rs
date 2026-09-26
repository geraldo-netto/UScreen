//! T448/T416: opt-in isolated measurement; no EVDI/display/ADB attachment.
use super::*;
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncWriteExt, BufReader};

fn command(encoder: &str, framed: bool, depth: bool) -> Command {
    let config = CaptureConfig {
        encoder: encoder.into(),
        width: 640,
        height: 400,
        fps: 60,
        instance: u32::MAX,
        ..Default::default()
    };
    let built = CliEncoder { config: &config }
        .encoder_command(640, 400, depth)
        .unwrap();
    let mut args = built
        .as_std()
        .get_args()
        .map(std::ffi::OsStr::to_owned)
        .collect::<Vec<_>>();
    let input = args.iter().position(|arg| arg == "-i").unwrap();
    args[input + 1] = "pipe:0".into();
    if !framed {
        args.truncate(args.len() - 3);
        args.extend(["-f", Codec::from_encoder(encoder).wire_name(), "pipe:1"].map(Into::into));
    }
    let mut command = Command::new("ffmpeg");
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command
}

async fn feed(
    mut stdin: tokio::process::ChildStdin,
    sent: Arc<std::sync::Mutex<Vec<Instant>>>,
    fps: u32,
    count: usize,
) {
    let pixels = 640 * 400;
    let mut frame = vec![128; pixels * 3 / 2];
    for (index, value) in frame[..pixels].iter_mut().enumerate() {
        *value = (16 + (((index / 640 / 8) ^ (index % 640 / 8)) * 37 % 220)) as u8;
    }
    let mut tick = tokio::time::interval(Duration::from_nanos(1_000_000_000 / u64::from(fps)));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    for index in 0..count {
        tick.tick().await;
        let row = index % 400;
        frame[row * 640..(row + 1) * 640].fill((16 + index * 3 % 220) as u8);
        sent.lock().unwrap().push(Instant::now());
        stdin.write_all(&frame).await.unwrap();
    }
    stdin.shutdown().await.unwrap();
}

async fn drain(
    stdout: tokio::process::ChildStdout,
    codec: Codec,
    framed: bool,
) -> Vec<(Instant, usize)> {
    let mut input = BufReader::new(stdout);
    let mut current = Packetizer::new(codec, Default::default());
    let mut legacy = crate::annex_b::AnnexBPacketizer::new(codec, Default::default());
    let mut received = Vec::new();
    loop {
        let (read, frames) = if framed {
            current.read_from(&mut input).await.unwrap()
        } else {
            legacy.read_from(&mut input).await.unwrap()
        };
        received.extend(
            frames
                .iter()
                .map(|frame| (Instant::now(), frame.data.len())),
        );
        if read == 0 {
            return received;
        }
    }
}

async fn sample(encoder: String, framed: bool, fps: u32, depth: bool) -> serde_json::Value {
    let count = if fps == 60 { 120 } else { 25 };
    let mut child = command(&encoder, framed, depth).spawn().unwrap();
    let sent = Arc::new(std::sync::Mutex::new(Vec::new()));
    let ((), received) = tokio::join!(
        feed(child.stdin.take().unwrap(), sent.clone(), fps, count),
        drain(
            child.stdout.take().unwrap(),
            Codec::from_encoder(&encoder),
            framed
        )
    );
    assert!(
        child.wait().await.unwrap().success(),
        "T448: {encoder} failed"
    );
    assert_eq!(received.len(), count, "T448: {encoder} lost pictures");
    let sent = sent.lock().unwrap();
    let latency = received
        .iter()
        .zip(sent.iter())
        .map(|((at, _), sent)| at.duration_since(*sent).as_micros() as u64)
        .collect::<Vec<_>>();
    let sizes = received.iter().map(|(_, size)| *size).collect::<Vec<_>>();
    json!({"latency_us":latency,"encoded_bytes":sizes})
}

fn cpu(who: libc::c_int) -> f64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // getrusage fills the entire output on success; no pointers are retained.
    assert_eq!(unsafe { libc::getrusage(who, usage.as_mut_ptr()) }, 0);
    let usage = unsafe { usage.assume_init() };
    let total = usage.ru_utime.tv_sec + usage.ru_stime.tv_sec;
    total as f64 + (usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) as f64 / 1e6
}

async fn batch(
    encoder: &str,
    framed: bool,
    fps: u32,
    sessions: usize,
    repeat: usize,
    depth: bool,
) -> serde_json::Value {
    let started = Instant::now();
    let host = cpu(libc::RUSAGE_SELF);
    let child = cpu(libc::RUSAGE_CHILDREN);
    let tasks = (0..sessions)
        .map(|_| tokio::spawn(sample(encoder.to_owned(), framed, fps, depth)))
        .collect::<Vec<_>>();
    let mut samples = Vec::new();
    for task in tasks {
        samples.push(task.await.unwrap());
    }
    json!({"encoder":encoder,"framed":framed,"fps":fps,"sessions":sessions,"repeat":repeat,
        "wall_s":started.elapsed().as_secs_f64(), "host_cpu_s":cpu(libc::RUSAGE_SELF)-host,
        "encoder_cpu_s":cpu(libc::RUSAGE_CHILDREN)-child,"samples":samples})
}

#[tokio::test]
#[ignore = "Opt-in T448/T416 timing experiment; deterministic regressions run normally"]
async fn t448_packet_framing_profile() {
    let encoder = std::env::var("BLENT_PROFILE_ENCODER").unwrap_or("libx264".into());
    let depth = supports_async_depth(
        std::ffi::OsStr::new("ffmpeg"),
        crate::config::ffmpeg_encoder_name(&encoder),
    )
    .await;
    let mut samples = Vec::new();
    for repeat in 0..3 {
        for sessions in [1, 2, 4] {
            for fps in [5, 60] {
                // Alternate order to expose warmup/drift rather than assume it away.
                for framed in [repeat % 2 == 0, repeat % 2 != 0] {
                    samples.push(batch(&encoder, framed, fps, sessions, repeat, depth).await);
                }
            }
        }
    }
    println!(
        "T448_PROFILE {}",
        json!({"encoder":encoder,"width":640,"height":400,"async_depth_supported":depth,"samples":samples})
    );
}
