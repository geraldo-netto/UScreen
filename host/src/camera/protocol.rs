//! Fixed, bounded camera uplink. Each connection owns one camera generation.
use anyhow::{ensure, Result};
use tokio::io::{AsyncRead, AsyncReadExt};

pub const MAGIC: &[u8; 8] = b"USCAM001";
pub const MAX_PACKET: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Header {
    pub lens: usize,
    pub rotation: u8,
}

pub async fn header(reader: &mut (impl AsyncRead + Unpin), token: &str) -> Result<Header> {
    let mut bytes = [0u8; 74];
    reader.read_exact(&mut bytes).await?;
    ensure!(&bytes[..8] == MAGIC, "unsupported camera protocol");
    let presented = std::str::from_utf8(&bytes[8..72])?;
    ensure!(
        uscreen_config::runtime::token_matches(token, presented),
        "camera authentication failed"
    );
    ensure!(bytes[72] <= 1, "invalid camera identity");
    ensure!(bytes[73] <= 3, "invalid camera rotation");
    Ok(Header {
        lens: bytes[72] as usize,
        rotation: bytes[73],
    })
}

pub async fn packet(reader: &mut (impl AsyncRead + Unpin)) -> Result<Vec<u8>> {
    let length = reader.read_u32().await? as usize;
    ensure!(
        (1..=MAX_PACKET).contains(&length),
        "invalid camera packet size"
    );
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).await?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn greeting() -> Vec<u8> {
        [MAGIC.as_slice(), "a".repeat(64).as_bytes(), &[0, 0]].concat()
    }

    #[tokio::test]
    async fn t539_header_identity_authentication_and_bounds() {
        let token = "a".repeat(64);
        for lens in 0..2 {
            for rotation in 0..4 {
                let mut bytes = greeting();
                bytes[72] = lens;
                bytes[73] = rotation;
                assert_eq!(
                    header(&mut bytes.as_slice(), &token).await.unwrap(),
                    Header {
                        lens: lens as usize,
                        rotation
                    }
                );
            }
        }
        for index in 0..74 {
            let mut bytes = greeting();
            bytes[index] = 255;
            assert!(header(&mut bytes.as_slice(), &token).await.is_err());
        }
        for length in 0..74 {
            assert!(header(&mut &greeting()[..length], &token).await.is_err());
        }
    }

    #[tokio::test]
    async fn t539_packet_rejects_truncation_zero_and_oversize() {
        for length in [0, MAX_PACKET + 1, u32::MAX as usize] {
            assert!(packet(&mut &(length as u32).to_be_bytes()[..])
                .await
                .is_err());
        }
        for length in 1..128u32 {
            let mut bytes = length.to_be_bytes().to_vec();
            bytes.resize(length as usize + 4, 7);
            assert_eq!(
                packet(&mut bytes.as_slice()).await.unwrap(),
                vec![7; length as usize]
            );
            bytes.pop();
            assert!(packet(&mut bytes.as_slice()).await.is_err());
        }
    }
}
