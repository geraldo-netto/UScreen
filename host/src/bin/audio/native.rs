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

pub fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 2,
        "Expected queue milliseconds and unique node name"
    );
    let mut profile = AudioProfile::new(Direction::Microphone);
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
    let epoch = Instant::now();
    let input_queue = queue.clone();
    let input_running = running.clone();
    std::thread::spawn(move || {
        let _ = input(&mut std::io::stdin().lock(), &input_queue, epoch);
        input_running.store(false, Ordering::Release);
    });
    device(&args[1], Render::new(queue, epoch), running)
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

fn device(name: &str, render: Render, running: Arc<AtomicBool>) -> Result<()> {
    pw::init();
    let mainloop = pw::main_loop::MainLoop::new(None)?;
    let context = pw::context::Context::new(&mainloop)?;
    let core = context.connect(None)?;
    let loop_for_disconnect = mainloop.clone();
    let _core_listener = core
        .add_listener_local()
        .error(move |_, _, _, _| loop_for_disconnect.quit())
        .register();
    let stream = pw::stream::Stream::new(
        &core,
        "Blent Microphone",
        properties! {
            "media.type" => "Audio", "media.category" => "Playback", "media.class" => "Audio/Source",
            "node.description" => "Blent Microphone", "node.name" => name,
            "node.virtual" => "true", "node.autoconnect" => "false", "priority.session" => "-10000",
            "node.latency" => "480/48000", "audio.rate" => "48000", "audio.channels" => "1",
        },
    )?;
    let _listener = stream_listener(&stream, &mainloop, render)?;
    let format = format()?;
    let mut params = [spa::pod::Pod::from_bytes(&format)
        .ok_or_else(|| anyhow::anyhow!("invalid audio format"))?];
    stream.connect(
        spa::utils::Direction::Output,
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

fn stream_listener(
    stream: &pw::stream::Stream,
    mainloop: &pw::main_loop::MainLoop,
    render: Render,
) -> Result<pw::stream::StreamListener<Render>> {
    let loop_for_error = mainloop.clone();
    let mut announced = false;
    Ok(stream
        .add_local_listener_with_user_data(render)
        .state_changed(move |_, _, _, state| {
            if matches!(state, pw::stream::StreamState::Error(_)) {
                loop_for_error.quit();
            }
            if state == pw::stream::StreamState::Paused && !announced {
                announced = true;
                let _ = std::io::stdout().write_all(b"READY\n");
            }
        })
        .io_changed(|_, render, id, pointer, size| render.position(id, pointer, size))
        .process(process)
        .register()?)
}

fn format() -> Result<Vec<u8>> {
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::S16LE);
    info.set_rate(48000);
    info.set_channels(1);
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

fn process(stream: &pw::stream::StreamRef, render: &mut Render) {
    let Some(mut buffer) = stream.dequeue_buffer() else {
        return;
    };
    let Some(data) = buffer.datas_mut().first_mut() else {
        return;
    };
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
