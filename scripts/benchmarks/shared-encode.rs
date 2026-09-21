// Appended inside an exact copy of encoder.rs, so private production entry
// points can be measured without widening the production API.
pub fn benchmark(args: &[String]) -> anyhow::Result<()> {
    use std::io::{BufRead, BufReader};
    use std::os::fd::FromRawFd;
    use std::io::Write;
    use std::process::Stdio;
    fn now() -> u64 {
        let mut stamp: libc::timespec = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut stamp) }, 0);
        stamp.tv_sec as u64 * 1_000_000_000 + stamp.tv_nsec as u64
    }
    let (width, height, frames, threads): (u32, u32, usize, u32) =
        (args[2].parse()?, args[3].parse()?, args[4].parse()?, args[5].parse()?);
    let fps = 60;
    let warmup = 60;
    let cooldown = 60;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("capture.fifo");
    let native = std::ffi::CString::new(path.to_str().unwrap())?;
    assert_eq!(unsafe { libc::mkfifo(native.as_ptr(), 0o600) }, 0);
    let (parent, child) = crate::raw_socket::Socket::pair()?;
    let mut command = tokio::process::Command::new(&args[0]);
    command.arg(&args[1]).arg(&path).args([width.to_string(), height.to_string(),
        (frames + warmup + cooldown).to_string(), fps.to_string(), threads.to_string()]);
    child.attach(&mut command);
    command.stdout(Stdio::piped()).stderr(Stdio::inherit());
    let mut producer = command.as_std_mut().spawn()?;
    // Drop the command too: its pre_exec closure owns the child endpoint.
    drop(command); drop(child);
    let socket = (args[1] == "shared").then_some(parent);
    let mut input = shared::Input::open(&path, socket, (width, height), 4)?;
    let stop = StopSignal::new()?;
    let mut encoder = Encoder::new("libx264", width, height, fps, 60000, 18)?;
    let mut rows = Vec::new();
    let mut encoded = Vec::new();
    let mut usb = args.get(7).map(|fd| unsafe { std::net::TcpStream::from_raw_fd(fd.parse().unwrap()) });
    for line in BufReader::new(producer.stdout.take().unwrap()).lines() {
        let line = line?;
        let fields: Vec<u64> = line.split_whitespace().map(|v| v.parse().unwrap()).collect();
        assert!(input.read(&mut encoder.frame, &stop)?);
        let input_ready = now();
        let packets = encoder.encode_prepared(fields[0] % fps as u64 == 0)?;
        let packet_ready = now();
        assert_eq!(packets.len(), 1, "fixed zero-B-frame replay must produce one access unit per frame");
        if let Some(connection) = usb.as_mut() {
            benchmark_send(connection, fields[0] as u32 + 1, &packets[0].0)?;
        }
        encoded.extend_from_slice(&packets[0].0);
        if (warmup as u64..(warmup + frames) as u64).contains(&fields[0]) {
            rows.push(serde_json::json!({"sequence": fields[0], "capture_ns": fields[1],
                "converted_ns": fields[2], "input_ready_ns": input_ready, "packet_ready_ns": packet_ready,
                "bytes": packets[0].0.len()}));
        }
    }
    assert!(producer.wait()?.success());
    assert_eq!(rows.len(), frames);
    if let Some(connection) = usb.as_mut() { connection.write_all(&2u32.to_be_bytes())?; }
    std::fs::write(&args[6], encoded)?;
    println!("{}", serde_json::json!({"frames": rows, "codec_version": ffmpeg_next::codec::version(),
        "configuration": ffmpeg_next::codec::configuration(), "mode":args[1], "width":width,
        "height":height,"fps":fps,"workers":threads}));
    Ok(())
}

fn benchmark_send(connection: &mut std::net::TcpStream, sequence: u32, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::{IoSlice, Write};
    let mut header = [0u8; 12];
    header[4..8].copy_from_slice(&sequence.to_be_bytes());
    header[8..12].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
    let mut slices = [IoSlice::new(&header), IoSlice::new(bytes)];
    let mut remaining = &mut slices[..];
    // Match production's vectored frame write while retaining replay framing.
    while !remaining.is_empty() {
        let count = connection.write_vectored(remaining)?;
        if count == 0 { return Err(std::io::ErrorKind::WriteZero.into()); }
        IoSlice::advance_slices(&mut remaining, count);
    }
    Ok(())
}
