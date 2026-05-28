use anyhow::Context;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::SigningKey;
use ed25519_dalek::pkcs8::DecodePrivateKey;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceClaims {
    /// Device id (numeric, serialized as string per JWT conventions).
    pub sub: String,
    pub serial: String,
    pub iss: String,
    pub iat: u64,
    pub exp: u64,
}

/// Holds the encoding + decoding key for the device-JWT signing keypair,
/// plus the precomputed JWK and kid that go into the JWKS endpoint.
#[derive(Clone)]
pub struct DeviceJwtSigner {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    kid: String,
    issuer: String,
    ttl_seconds: u64,
    public_key_b64url: String,
}

impl std::fmt::Debug for DeviceJwtSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJwtSigner")
            .field("kid", &self.kid)
            .field("issuer", &self.issuer)
            .field("ttl_seconds", &self.ttl_seconds)
            .finish()
    }
}

impl DeviceJwtSigner {
    pub fn new(private_key_pem: &str, issuer: String, ttl_seconds: u64) -> anyhow::Result<Self> {
        let signing_key = SigningKey::from_pkcs8_pem(private_key_pem)
            .context("failed to parse DEVICE_JWT_PRIVATE_KEY_PEM as Ed25519 PKCS8 PEM")?;
        let verifying_key = signing_key.verifying_key();
        let public_bytes = verifying_key.to_bytes();

        let encoding_key = EncodingKey::from_ed_pem(private_key_pem.as_bytes())
            .context("jsonwebtoken rejected the Ed25519 private key PEM")?;
        let decoding_key = DecodingKey::from_ed_der(&public_bytes);

        let public_key_b64url = URL_SAFE_NO_PAD.encode(public_bytes);

        // kid = first 8 bytes of sha256(pubkey), hex. Deterministic per key.
        let digest = Sha256::digest(public_bytes);
        let kid = hex_encode(&digest[..8]);

        Ok(Self {
            encoding_key,
            decoding_key,
            kid,
            issuer,
            ttl_seconds,
            public_key_b64url,
        })
    }

    pub fn ttl_seconds(&self) -> u64 {
        self.ttl_seconds
    }

    pub fn mint(&self, device_id: i32, serial: &str) -> anyhow::Result<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock before UNIX epoch")?
            .as_secs();

        let claims = DeviceClaims {
            sub: device_id.to_string(),
            serial: serial.to_string(),
            iss: self.issuer.clone(),
            iat: now,
            exp: now + self.ttl_seconds,
        };

        let mut header = Header::new(Algorithm::EdDSA);
        header.kid = Some(self.kid.clone());

        encode(&header, &claims, &self.encoding_key).context("failed to encode device JWT")
    }

    pub fn verify(&self, token: &str) -> anyhow::Result<DeviceClaims> {
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.set_issuer(&[&self.issuer]);
        validation.set_required_spec_claims(&["exp", "iss", "sub"]);

        let data = decode::<DeviceClaims>(token, &self.decoding_key, &validation)
            .context("device JWT verification failed")?;
        Ok(data.claims)
    }

    /// JWKS document with the single active public key. Suitable for
    /// `/.well-known/jwks.json`.
    pub fn jwks(&self) -> serde_json::Value {
        serde_json::json!({
            "keys": [{
                "kty": "OKP",
                "crv": "Ed25519",
                "alg": "EdDSA",
                "use": "sig",
                "kid": self.kid,
                "x": self.public_key_b64url,
            }]
        })
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}
