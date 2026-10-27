use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

pub struct UploadTokenKey(Vec<u8>);

impl std::fmt::Debug for UploadTokenKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UploadTokenKey").finish_non_exhaustive()
    }
}

#[derive(Serialize, Deserialize)]
pub struct UploadClaims {
    pub device_id: i32,
    pub bucket: String,
    pub key: String,
    pub upload_id: String,
    pub size_bytes: i64,
    pub checksum: String,
    pub expires_at: i64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TokenError {
    Invalid,
    Expired,
}

impl UploadTokenKey {
    pub fn new(secret: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(
            secret.len() >= 32,
            "UPLOAD_TOKEN_SECRET must contain at least 32 bytes"
        );
        Ok(Self(secret.as_bytes().to_vec()))
    }

    pub fn sign(&self, claims: &UploadClaims) -> Result<String, serde_json::Error> {
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims)?);
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.0).expect("HMAC accepts any key length");
        mac.update(payload.as_bytes());
        Ok(format!(
            "{payload}.{}",
            URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
        ))
    }

    pub fn verify(
        &self,
        token: &str,
        device_id: i32,
        now: i64,
    ) -> Result<UploadClaims, TokenError> {
        if token.len() > 8192 {
            return Err(TokenError::Invalid);
        }
        let (payload, signature) = token.split_once('.').ok_or(TokenError::Invalid)?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| TokenError::Invalid)?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.0).expect("HMAC accepts any key length");
        mac.update(payload.as_bytes());
        mac.verify_slice(&signature)
            .map_err(|_| TokenError::Invalid)?;
        let payload = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| TokenError::Invalid)?;
        let claims: UploadClaims =
            serde_json::from_slice(&payload).map_err(|_| TokenError::Invalid)?;
        if claims.device_id != device_id {
            return Err(TokenError::Invalid);
        }
        if claims.expires_at <= now {
            return Err(TokenError::Expired);
        }
        Ok(claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_binds_device_upload_and_expiry() {
        let key = UploadTokenKey::new(&"a".repeat(32)).unwrap();
        let claims = UploadClaims {
            device_id: 1,
            bucket: "assets".into(),
            key: "clips/a".into(),
            upload_id: "id/+".into(),
            size_bytes: 42,
            checksum: "b".repeat(64),
            expires_at: 100,
        };
        let token = key.sign(&claims).unwrap();
        let verified = key.verify(&token, 1, 99).unwrap();
        assert_eq!(verified.upload_id, claims.upload_id);
        assert!(matches!(
            key.verify(&token, 2, 99),
            Err(TokenError::Invalid)
        ));
        assert!(matches!(
            key.verify(&token, 1, 100),
            Err(TokenError::Expired)
        ));
        let (payload, signature) = token.split_once('.').unwrap();
        let mut altered: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).unwrap()).unwrap();
        for field in [
            "device_id",
            "bucket",
            "key",
            "upload_id",
            "size_bytes",
            "checksum",
            "expires_at",
        ] {
            let original = altered[field].clone();
            altered[field] = serde_json::json!("changed");
            let changed = format!(
                "{}.{signature}",
                URL_SAFE_NO_PAD.encode(serde_json::to_vec(&altered).unwrap())
            );
            assert!(matches!(
                key.verify(&changed, 1, 99),
                Err(TokenError::Invalid)
            ));
            altered[field] = original;
        }
        assert!(UploadTokenKey::new("short").is_err());
        assert!(!format!("{key:?}").contains(&"a".repeat(32)));
    }
}
