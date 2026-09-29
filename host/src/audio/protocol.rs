use anyhow::{ensure, Result};
use blent_config::audio::{AudioCapabilities, AudioGrant, Direction, Processing};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

pub(super) async fn accept(
    listener: &TcpListener,
    token: &str,
    direction: Direction,
) -> Result<(TcpStream, AudioCapabilities, Processing, String)> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    loop {
        let (mut socket, _) = tokio::time::timeout_at(deadline, listener.accept()).await??;
        socket.set_nodelay(true)?;
        let request_deadline = (tokio::time::Instant::now() + Duration::from_secs(2)).min(deadline);
        let result =
            tokio::time::timeout_at(request_deadline, request(&mut socket, token, direction)).await;
        if let Ok(Ok((caps, mode, detail))) = result {
            return Ok((socket, caps, mode, detail));
        }
        ensure!(
            tokio::time::Instant::now() < deadline,
            "Audio consent timed out"
        );
    }
}
async fn request(
    socket: &mut TcpStream,
    token: &str,
    direction: Direction,
) -> Result<(AudioCapabilities, Processing, String)> {
    let mut bytes = [0; 76];
    socket.read_exact(&mut bytes).await?;
    ensure!(&bytes[..8] == b"BLAUREQ1", "Unknown audio request");
    ensure!(
        blent_config::credentials::token_matches(token, std::str::from_utf8(&bytes[8..72])?),
        "Audio request authentication failed"
    );
    ensure!(
        bytes[72] <= 7
            && bytes[73] <= 1
            && bytes[74] == direction as u8
            && matches!(bytes[75], 1 | 2),
        "Invalid audio capabilities"
    );
    let capabilities = AudioCapabilities {
        microphone: direction == Direction::Microphone,
        speakers: direction == Direction::Speakers,
        speech: bytes[72] & 1 != 0,
        raw: bytes[72] & 2 != 0,
        background: bytes[72] & 4 != 0,
    };
    let detail = if bytes[73] != 0 {
        "Speech AEC enabled; acoustic effectiveness is device-dependent."
    } else {
        "AEC unavailable or raw processing selected."
    };
    let processing = if bytes[75] == 1 {
        Processing::Speech
    } else {
        Processing::Raw
    };
    Ok((
        capabilities,
        processing,
        format!("Effective {processing:?}. {detail}"),
    ))
}
pub(super) async fn grant(socket: &mut TcpStream, ticket: &str, grant: &AudioGrant) -> Result<()> {
    let bytes = [ticket.as_bytes(), grant.hello().as_slice()].concat();
    tokio::time::timeout(Duration::from_secs(2), socket.write_all(&bytes)).await??;
    Ok(())
}

/// One deadline belongs around this whole operation, including partial reads.
pub(super) async fn packet(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    frames: &mut blent_config::audio::FrameReader,
) -> Result<Vec<u8>> {
    let mut header = [0; blent_config::audio::FRAME_HEADER_BYTES];
    reader.read_exact(&mut header).await?;
    let size = frames.payload_bytes(&header)?;
    let mut packet = vec![0; header.len() + size];
    packet[..header.len()].copy_from_slice(&header);
    reader.read_exact(&mut packet[header.len()..]).await?;
    let block = frames.decode(&packet)?;
    let mut native = Vec::with_capacity(1 + size);
    native.push(u8::from(block.discontinuity));
    native.extend_from_slice(&packet[header.len()..]);
    Ok(native)
}
