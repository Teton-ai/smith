//! Debug-access AP credential derivation.
//!
//! Mirrors the on-device derivation in plex
//! (`plex/src/debug_access/mod.rs::derive_password`) so the backend — which
//! already knows each device's `token` — can reproduce the WPA2 password for a
//! device's `<serial>-tunnel` debug AP without a device round-trip.
//!
//! This is the ONLY place the backend uses `device.token` to produce a
//! credential; the token itself is never returned to a client. HMAC-SHA256
//! cannot be reversed to recover the token, and the derivation depends on the
//! token being high-entropy (it is: a backend-issued credential).
//!
//! The algorithm MUST stay byte-for-byte identical to plex's, or the device and
//! the backend will disagree on the password. The known-answer vectors below
//! are copied verbatim from plex and pin that contract.

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Domain-separation label mixed into the debug-password HMAC. Must match
/// plex's `PASSWORD_DOMAIN`. Bump the version suffix in BOTH places to force a
/// fleet-wide password change.
const PASSWORD_DOMAIN: &[u8] = b"plex-debug-ap:v1:";

/// The WiFi SSID advertised while debug access is active: `<serial>-tunnel`.
pub fn ssid_for(serial: &str) -> String {
    format!("{serial}-tunnel")
}

/// Derive the WPA2 password from the device serial and the device `token`.
///
/// Algorithm (identical to plex):
///   1. `tag = HMAC-SHA256(key = token, msg = "plex-debug-ap:v1:" ++ serial)`
///   2. Take the first 7 tag bytes as a big-endian integer (56 bits).
///   3. Emit 10 chars: the top 50 bits, 5 bits per char (high bits first),
///      indexing the Crockford base32 alphabet (excludes i, l, o, u).
pub fn derive_debug_ap_password(serial: &str, token: &str) -> String {
    // Crockford base32 alphabet, lowercase. Exactly 32 symbols; excludes the
    // ambiguity-prone letters i, l, o, u.
    const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

    let mut mac = HmacSha256::new_from_slice(token.as_bytes())
        .expect("HMAC-SHA256 accepts a key of any length");
    mac.update(PASSWORD_DOMAIN);
    mac.update(serial.as_bytes());
    let tag = mac.finalize().into_bytes(); // 32 bytes

    // First 56 bits, big-endian; we consume the top 50 (10 chars * 5 bits).
    let mut acc: u64 = 0;
    for &b in &tag[..7] {
        acc = (acc << 8) | b as u64;
    }

    let mut out = String::with_capacity(10);
    for i in 0..10 {
        let idx = ((acc >> (5 * (9 - i))) & 0x1f) as usize;
        out.push(ALPHABET[idx] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Same test token plex uses in its unit tests.
    const TEST_TOKEN: &str =
        "8f3a1c9e4b7d2f60a5c8e1b3d9f4a2c6e8b0d1f3a5c7e9b2d4f6a8c0e2b4d6f8";

    #[test]
    fn password_is_deterministic_and_wpa2_valid() {
        let a = derive_debug_ap_password("1421325026425", TEST_TOKEN);
        let b = derive_debug_ap_password("1421325026425", TEST_TOKEN);
        assert_eq!(a, b, "same serial+token must yield the same password");
        assert_eq!(a.len(), 10);
        assert!((8..=63).contains(&a.len()), "WPA2-PSK requires 8..=63 chars");
        assert!(
            a.chars().all(|c| c.is_ascii_alphanumeric()),
            "password must be printable ascii alphanumerics"
        );
    }

    /// Known-answer vectors copied verbatim from plex. If these ever diverge,
    /// the backend and the device disagree on the password.
    #[test]
    fn password_matches_plex_reference_vector() {
        assert_eq!(
            derive_debug_ap_password("1421325026425", TEST_TOKEN),
            "xk9r5h3gk6"
        );
        assert_eq!(
            derive_debug_ap_password("0000000000001", TEST_TOKEN),
            "f9gdde09bk"
        );
    }

    #[test]
    fn password_varies_by_serial_and_token() {
        let base = derive_debug_ap_password("1421325026425", TEST_TOKEN);
        assert_ne!(base, derive_debug_ap_password("1421325026426", TEST_TOKEN));
        assert_ne!(
            base,
            derive_debug_ap_password("1421325026425", "a-different-token")
        );
    }

    #[test]
    fn ssid_format() {
        assert_eq!(ssid_for("1421325026425"), "1421325026425-tunnel");
    }
}
