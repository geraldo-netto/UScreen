//! Portable attachment credentials. Native entropy comes from stock getrandom.
use anyhow::Result;

/// 64 hexadecimal characters from the operating system CSPRNG; never persisted here.
pub fn random_token() -> Result<String> {
    random_token_using(getrandom::fill)
}

fn random_token_using(fill: fn(&mut [u8]) -> Result<(), getrandom::Error>) -> Result<String> {
    let mut raw = [0u8; 32];
    fill(&mut raw)
        .map_err(|error| anyhow::anyhow!("operating system entropy unavailable: {error}"))?;
    Ok(raw.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Compare a presented token with the expected one. Constant-time over the
/// expected length, so timing does not leak how many leading characters were
/// right — cheap insurance on a loopback socket.
pub fn token_matches(expected: &str, presented: &str) -> bool {
    let a = expected.as_bytes();
    let b = presented.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t523_entropy_failure_never_becomes_an_empty_credential() {
        assert!(random_token_using(|_| Err(getrandom::Error::UNSUPPORTED)).is_err());
        let token = random_token_using(|bytes| {
            bytes.fill(0xa5);
            Ok(())
        })
        .unwrap();
        assert_eq!(token, "a5".repeat(32));
        for length in 0..=128 {
            assert!(!token_matches(&token, &"x".repeat(length)));
        }
        assert!(token_matches(&token, &token));
        assert_ne!(random_token().unwrap(), random_token().unwrap());
    }
}
