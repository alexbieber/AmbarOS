//! Local IPC authentication and authorization.
//!
//! Three independent checks, all of which must pass:
//!
//! 1. **Filesystem**: the socket lives in a 0700 directory owned by the
//!    service user, and is itself 0600. Other users cannot connect.
//! 2. **Peer credentials**: on accept, the kernel-reported uid of the
//!    connecting process (`SO_PEERCRED` / `getpeereid`) must equal the
//!    service's own uid. Root is not exempt.
//! 3. **Token**: a fresh 256-bit secret is written to a 0600 file at every
//!    start; the client must present it in `Hello`. A token from a previous
//!    run (e.g. copied before a restart) is useless.
//!
//! Authorization: an authenticated client is the service's own user and may
//! use every operation. Finer-grained roles (e.g. read-only clients for
//! agents) are planned for the agent runtime phase.

use std::path::Path;

use crate::paths::{PathError, write_private_file};

/// Token length in bytes before hex encoding.
pub const TOKEN_BYTES: usize = 32;

/// Generates a new random token (hex-encoded).
pub fn generate_token() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Writes the token file (mode 0600, atomic replace).
pub fn write_token(path: &Path, token: &str) -> Result<(), PathError> {
    write_private_file(path, token.as_bytes())
}

/// Reads the token file.
pub fn read_token(path: &Path) -> std::io::Result<String> {
    Ok(std::fs::read_to_string(path)?.trim().to_string())
}

/// Compares secrets without leaking the position of the first difference.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Decides whether a connecting peer may talk to the service.
pub fn authorize_peer(peer_uid: u32, service_uid: u32) -> Result<(), String> {
    if peer_uid == service_uid {
        Ok(())
    } else {
        Err(format!(
            "connection from uid {peer_uid} refused: service runs as uid {service_uid}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_and_long() -> Result<(), getrandom::Error> {
        let a = generate_token()?;
        let b = generate_token()?;
        assert_eq!(a.len(), TOKEN_BYTES * 2);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        Ok(())
    }

    #[test]
    fn comparison() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(!constant_time_eq("", "a"));
    }

    #[test]
    fn peers_must_be_the_same_user() {
        assert!(authorize_peer(501, 501).is_ok());
        assert!(authorize_peer(0, 501).is_err(), "root is not exempt");
        assert!(authorize_peer(502, 501).is_err());
    }
}
