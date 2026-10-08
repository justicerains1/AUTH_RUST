//! Read-only metrics authentication; secret files never become labels or logs.
use crate::security::{constant_time_equal, token_digest};
use std::{fs, path::Path};
use zeroize::Zeroizing;

pub fn load_metrics_digest(path: &Path, production: bool) -> Result<[u8; 32], &'static str> {
    let metadata = fs::metadata(path).map_err(|_| "invalid METRICS_TOKEN_FILE")?;
    if !metadata.is_file() || metadata.len() > 1024 {
        return Err("invalid METRICS_TOKEN_FILE");
    }
    #[cfg(unix)]
    if production {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("invalid METRICS_TOKEN_FILE");
        }
    }
    #[cfg(not(unix))]
    let _ = production;
    let text = Zeroizing::new(fs::read_to_string(path).map_err(|_| "invalid METRICS_TOKEN_FILE")?);
    let token = text.trim_end();
    if token.len() < 43
        || token.len() > 512
        || !token
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || b"-_".contains(&value))
    {
        return Err("invalid METRICS_TOKEN_FILE");
    }
    Ok(token_digest(token))
}
pub fn metrics_authorized(
    peer: std::net::IpAddr,
    authorization: Option<&str>,
    duplicate: bool,
    expected: Option<&[u8; 32]>,
) -> bool {
    let Some(expected) = expected else {
        return peer.is_loopback();
    };
    if duplicate {
        return false;
    }
    let Some(header) = authorization else {
        return false;
    };
    let Some(token) = header.strip_prefix("Bearer ") else {
        return false;
    };
    token.len() <= 512 && constant_time_equal(expected, &token_digest(token))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metrics_authentication_is_explicit_and_has_no_cookie_fallback() {
        let peer = "192.0.2.1"
            .parse()
            .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
        let token = "A".repeat(43);
        let digest = token_digest(&token);
        assert!(!metrics_authorized(peer, None, false, None));
        assert!(metrics_authorized(
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            None,
            false,
            None
        ));
        assert!(!metrics_authorized(peer, None, false, Some(&digest)));
        assert!(metrics_authorized(
            peer,
            Some(&format!("Bearer {token}")),
            false,
            Some(&digest)
        ));
        assert!(!metrics_authorized(
            peer,
            Some(&format!("Bearer {token}")),
            true,
            Some(&digest)
        ));
        assert!(!metrics_authorized(
            peer,
            Some("Bearer wrong"),
            false,
            Some(&digest)
        ));
    }
}
