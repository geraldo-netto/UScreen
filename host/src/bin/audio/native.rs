use anyhow::{ensure, Result};
use blent_config::audio::{AudioProfile, Direction, PcmQueue, BLOCK_FRAMES};
use pipewire as pw;
use pw::{properties::properties, spa};
use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

#[path = "ring.rs"]
mod ring;
use ring::Render;
#[path = "capture.rs"]
mod capture;
use capture::Capture;

enum DeviceIo {
    Microphone(Render),
    Speakers(Capture),
}

pub fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        (2..=3).contains(&args.len()),
        "Expected queue milliseconds, unique node name and optional direction"
    );
    let direction = match args.get(2).map(String::as_str).unwrap_or("microphone") {
        "microphone" => Direction::Microphone,
        "speakers" => Direction::Speakers,
        _ => anyhow::bail!("Invalid audio direction"),
    };
    let mut profile = AudioProfile::new(direction);
    profile.buffer_ms = args[0].parse()?;
    profile.validate()?;
    ensure!(
        args[1]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "Invalid node name"
    );
    let queue = Arc::new(Mutex::new(PcmQueue::new(profile)?));
    let running = Arc::new(AtomicBool::new(true));
    let ready = Arc::new(AtomicBool::new(false));
    let epoch = Instant::now();
    let io = match direction {
        Direction::Microphone => {
            input_worker(queue.clone(), epoch, running.clone());
            DeviceIo::Microphone(Render::new(queue, epoch))
        }
        Direction::Speakers => {
            capture::workers(queue.clone(), epoch, running.clone(), ready.clone());
            DeviceIo::Speakers(Capture::new(queue, epoch))
        }
    };
    let result = device(&args[1], direction, io, running.clone(), ready);
    running.store(false, Ordering::Release);
    result
}

fn input_worker(queue: Arc<Mutex<PcmQueue>>, epoch: Instant, running: Arc<AtomicBool>) {
    let input_queue = queue.clone();
    std::thread::spawn(move || {
        let _ = input(&mut std::io::stdin().lock(), &input_queue, epoch);
        running.store(false, Ordering::Release);
    });
}

fn input(reader: &mut impl Read, queue: &Mutex<PcmQueue>, epoch: Instant) -> Result<()> {
    let mut bytes = [0; 1 + BLOCK_FRAMES * 2];
    loop {
        reader.read_exact(&mut bytes)?;
        ensure!(bytes[0] <= 1, "Invalid native discontinuity flag");
        let mut block =
            blent_config::audio::PcmBlock::from_le_bytes(Direction::Microphone, &bytes[1..])?;
        block.discontinuity = bytes[0] != 0;
        queue
            .lock()
            .map_err(|_| anyhow::anyhow!("audio queue poisoned"))?
            .push(block, epoch.elapsed().as_millis() as u64)?;
    }
}

fn device(
    name: &str,
    direction: Direction,
    io: DeviceIo,
    running: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
) -> Result<()> {
    pw::init();
    let mainloop = pw::main_loop::MainLoop::new(None)?;
    let context = pw::context::Context::new(&mainloop)?;
    let core = context.connect(None)?;
    let loop_for_disconnect = mainloop.clone();
    let _core_listener = core
        .add_listener_local()
        .error(move |_, _, _, _| loop_for_disconnect.quit())
        .register();
    let stream = create_stream(&core, name, direction)?;
    let _listener = stream_listener(&stream, &mainloop, io, ready)?;
    let format = format(direction)?;
    let mut params = [spa::pod::Pod::from_bytes(&format)
        .ok_or_else(|| anyhow::anyhow!("invalid audio format"))?];
    stream.connect(
        if direction == Direction::Microphone {
            spa::utils::Direction::Output
        } else {
            spa::utils::Direction::Input
        },
        None,
        pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::RT_PROCESS,
        &mut params,
    )?;
    let loop_for_stop = mainloop.clone();
    let timer = mainloop.loop_().add_timer(move |_| {
        if !running.load(Ordering::Acquire) {
            loop_for_stop.quit();
        }
    });
    timer
        .update_timer(
            Some(Duration::from_millis(10)),
            Some(Duration::from_millis(10)),
        )
        .into_result()?;
    mainloop.run();
    Ok(())
}

fn create_stream(
    core: &pw::core::Core,
    name: &str,
    direction: Direction,
) -> Result<pw::stream::Stream> {
    let (label, category, class) = match direction {
        Direction::Microphone => ("Blent Microphone", "Playback", "Audio/Source"),
        Direction::Speakers => ("Blent Speakers", "Capture", "Audio/Sink"),
    };
    Ok(pw::stream::Stream::new(
        core,
        label,
        properties! {
            "media.type" => "Audio", "media.category" => category, "media.class" => class,
            "node.description" => label, "node.name" => name,
            "node.virtual" => "true", "node.autoconnect" => "false", "priority.session" => "-10000",
            "node.latency" => "480/48000", "audio.rate" => "48000", "audio.channels" => direction.channels().to_string(),
        },
    )?)
}

fn stream_listener(
    stream: &pw::stream::Stream,
    mainloop: &pw::main_loop::MainLoop,
    io: DeviceIo,
    ready: Arc<AtomicBool>,
) -> Result<pw::stream::StreamListener<DeviceIo>> {
    let loop_for_error = mainloop.clone();
    let mut announced = false;
    Ok(stream
        .add_local_listener_with_user_data(io)
        .state_changed(move |_, _, _, state| {
            if matches!(state, pw::stream::StreamState::Error(_)) {
                loop_for_error.quit();
            }
            if state == pw::stream::StreamState::Paused && !announced {
                announced = true;
                let _ = std::io::stdout().write_all(b"READY\n");
                ready.store(true, Ordering::Release);
            }
        })
        .io_changed(|_, io, id, pointer, size| {
            if let DeviceIo::Microphone(render) = io {
                render.position(id, pointer, size);
            }
        })
        .process(process)
        .register()?)
}

fn format(direction: Direction) -> Result<Vec<u8>> {
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::S16LE);
    info.set_rate(48000);
    info.set_channels(direction.channels() as u32);
    if direction == Direction::Speakers {
        let mut positions = [0; 64];
        positions[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
        positions[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
        info.set_position(positions);
    }
    let value = spa::pod::Value::Object(spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    });
    Ok(
        spa::pod::serialize::PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &value)?
            .0
            .into_inner(),
    )
}

fn process(stream: &pw::stream::StreamRef, io: &mut DeviceIo) {
    let Some(mut buffer) = stream.dequeue_buffer() else {
        return;
    };
    let Some(data) = buffer.datas_mut().first_mut() else {
        return;
    };
    match io {
        DeviceIo::Microphone(render) => produce(data, render),
        DeviceIo::Speakers(capture) => consume(data, capture),
    }
}

fn consume(data: &mut spa::buffer::Data, capture: &mut Capture) {
    let chunk = data.chunk();
    let (offset, size, stride, flags) = (
        chunk.offset(),
        chunk.size().min(data.as_raw().maxsize),
        chunk.stride(),
        chunk.flags().bits(),
    );
    // SPA EMPTY is a neutral block even if the backing memory still contains
    // old samples. Reject corrupted/unknown flags before touching that memory.
    if flags & !2 != 0 || !matches!(stride, 0 | 4) || size > 38400 || size % 4 != 0 {
        capture.discard();
    } else if flags == 2 {
        capture.silence(size);
    } else if let Some(bytes) = data.data() {
        consume_bytes(bytes, offset, size, stride, capture);
    } else {
        capture.discard();
    }
}

fn consume_bytes(bytes: &[u8], offset: u32, size: u32, stride: i32, capture: &mut Capture) {
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        capture.discard();
        return;
    }
    let start = offset as usize % bytes.len();
    let first = (size as usize).min(bytes.len() - start);
    capture.chunk(bytes, start as u32, first as u32, stride);
    capture.chunk(bytes, 0, size - first as u32, stride);
}

fn produce(data: &mut spa::buffer::Data, render: &mut Render) {
    let size = match data.data() {
        Some(bytes) => {
            let count = render.quantum(bytes.len());
            render.fill(&mut bytes[..count]);
            count
        }
        None => 0,
    };
    let chunk = data.chunk_mut();
    *chunk.offset_mut() = 0;
    *chunk.stride_mut() = 2;
    *chunk.size_mut() = size as u32;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t719_native_callback_without_a_buffer_returns_without_capturing() {
        pw::init();
        let mainloop = pw::main_loop::MainLoop::new(None).unwrap();
        let context = pw::context::Context::new(&mainloop).unwrap();
        // An owned socket pair provides no graph and cannot reach host devices.
        let (client, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
        let core = context.connect_fd(client.into(), None).unwrap();
        let stream = create_stream(&core, "blent_t719_unconnected", Direction::Speakers).unwrap();
        let queue = Arc::new(Mutex::new(
            PcmQueue::new(AudioProfile::new(Direction::Speakers)).unwrap(),
        ));
        let mut io = DeviceIo::Speakers(Capture::new(queue.clone(), Instant::now()));
        process(&stream, &mut io);
        assert_eq!(queue.lock().unwrap().queued_frames(), 0);
    }
    fn consume_fixture(
        capture: &mut Capture,
        bytes: &mut [u8],
        offset: u32,
        size: u32,
        flags: i32,
    ) {
        let mut chunk = spa::sys::spa_chunk {
            offset,
            size,
            stride: 4,
            flags,
        };
        let mut raw = spa::sys::spa_data {
            type_: spa::sys::SPA_DATA_MemPtr,
            flags: spa::sys::SPA_DATA_FLAG_READABLE,
            fd: -1,
            mapoffset: 0,
            maxsize: bytes.len() as u32,
            data: bytes.as_mut_ptr().cast(),
            chunk: &mut chunk,
        };
        // Data is repr(transparent); both backing slices outlive this callback.
        let data =
            unsafe { &mut *(&mut raw as *mut spa::sys::spa_data).cast::<spa::buffer::Data>() };
        consume(data, capture);
    }
    #[test]
    fn t719_native_chunk_flags_and_wrapping_preserve_valid_stereo_only() {
        let queue = Arc::new(Mutex::new(
            PcmQueue::new(AudioProfile::new(Direction::Speakers)).unwrap(),
        ));
        let epoch = Instant::now();
        let mut capture = Capture::new(queue.clone(), epoch);
        let mut bytes = [1; 1920];
        for flags in [-1, 1, 3, 4, i32::MIN, i32::MAX] {
            consume_fixture(&mut capture, &mut bytes, 0, 1920, flags);
            assert_eq!(
                queue.lock().unwrap().queued_frames(),
                0,
                "T719 corrupted/unknown SPA samples published"
            );
        }
        consume_fixture(&mut capture, &mut [], 0, 0, 0);
        consume_fixture(&mut capture, &mut [0; 5], 0, 4, 0);
        assert_eq!(queue.lock().unwrap().queued_frames(), 0);
        consume_fixture(&mut capture, &mut bytes, 0, 1920, 2);
        let empty = queue
            .lock()
            .unwrap()
            .pop(epoch.elapsed().as_millis() as u64)
            .unwrap()
            .unwrap();
        assert!(
            empty.samples().iter().all(|sample| *sample == 0),
            "T719 EMPTY chunk replayed stale PCM"
        );
        consume_fixture(&mut capture, &mut bytes, 1920, u32::MAX, 0);
        let wrapped = queue
            .lock()
            .unwrap()
            .pop(epoch.elapsed().as_millis() as u64)
            .unwrap()
            .unwrap();
        assert!(wrapped.samples().iter().all(|sample| *sample == 0x0101));
        let mut ring = [1; 3840];
        ring[1920..].fill(2);
        consume_fixture(&mut capture, &mut ring, 2880, 1920, 0);
        let split = queue
            .lock()
            .unwrap()
            .pop(epoch.elapsed().as_millis() as u64)
            .unwrap()
            .unwrap();
        assert!(split.samples()[..480]
            .iter()
            .all(|sample| *sample == 0x0202));
        assert!(split.samples()[480..]
            .iter()
            .all(|sample| *sample == 0x0101));
    }
    #[test]
    fn t718_native_input_validates_flags_and_flushes_discontinuity() {
        let queue = Mutex::new(PcmQueue::new(AudioProfile::new(Direction::Microphone)).unwrap());
        let epoch = Instant::now();
        let mut bytes = Vec::new();
        for flag in [0, 0, 1] {
            bytes.push(flag);
            bytes.extend_from_slice(&[1; BLOCK_FRAMES * 2]);
        }
        assert!(input(&mut bytes.as_slice(), &queue, epoch).is_err()); // EOF ends the worker.
        assert_eq!(queue.lock().unwrap().queued_frames(), BLOCK_FRAMES);
        for flag in 2..=255 {
            let mut bad = [0; 961];
            bad[0] = flag;
            assert!(input(&mut bad.as_slice(), &queue, epoch).is_err());
        }
        assert!(input(&mut [0; 400].as_slice(), &queue, epoch).is_err());
    }
}
