use anyhow::{Result, anyhow};
use base64::Engine;
use keyring::Entry;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use crate::config::Config;

#[derive(Serialize, Deserialize)]
struct DeviceAuthResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: String,
    expires_in: usize,
    interval: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenResponse {
    access_token: Option<String>,
    token_type: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<usize>,
    scope: Option<String>,
}

pub async fn login(config: &Config, open: bool) -> anyhow::Result<()> {
    let already_logged_in = get_secrets(config).await?;

    if already_logged_in.is_some() {
        println!("Already logged in");
        return Ok(());
    }

    let client = Client::new();

    let (domain, client_id, audience) = config.auth0_credentials();

    let resp = client
        .post(format!("https://{domain}/oauth/device/code"))
        .form(&[
            ("client_id", client_id.clone()),
            ("audience", audience),
            (
                "scope",
                String::from("openid profile offline_access smith:admin"),
            ),
        ])
        .send();

    let device_auth_response: DeviceAuthResponse = resp.await?.json::<DeviceAuthResponse>().await?;

    println!(
        "Go to {} and enter the code: {}",
        device_auth_response.verification_uri, device_auth_response.user_code
    );

    if open {
        open::that(device_auth_response.verification_uri_complete)?;
    }

    let token_endpoint = format!("https://{}/oauth/token", domain);

    // Polling for token.
    loop {
        let resp: TokenResponse = client
            .post(&token_endpoint)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", &device_auth_response.device_code),
                ("client_id", &client_id),
            ])
            .send()
            .await?
            .json::<TokenResponse>()
            .await?;

        println!("{:?}", resp);

        if let (Some(access_token), Some(refresh_token)) = (resp.access_token, resp.refresh_token) {
            let current_profile = config.current_profile.clone();

            // Try to get existing secrets from either storage
            let mut session_secrets = match try_keyring_get() {
                Ok(Some(json)) => serde_json::from_str::<SessionSecrets>(&json)?,
                Ok(None) => {
                    // Try file storage
                    match try_file_get()? {
                        Some(json) => serde_json::from_str::<SessionSecrets>(&json)?,
                        None => SessionSecrets::default(),
                    }
                }
                Err(_) => {
                    // Try file storage
                    match try_file_get()? {
                        Some(json) => serde_json::from_str::<SessionSecrets>(&json)?,
                        None => SessionSecrets::default(),
                    }
                }
            };

            let new_profile_secrets = ProfileSecrets {
                access_token,
                refresh_token,
            };

            if let Some(profile) = session_secrets.profiles.get_mut(&current_profile) {
                *profile = new_profile_secrets;
            } else {
                session_secrets
                    .profiles
                    .insert(current_profile, new_profile_secrets);
            }

            // Try keyring first, fall back to file
            let json = serde_json::to_string(&session_secrets)?;
            match try_keyring_set(&json) {
                Ok(()) => {
                    // Success - keyring available
                }
                Err(e) => {
                    // Keyring unavailable, use file storage
                    eprintln!("Warning: System keyring unavailable ({})", e);
                    eprintln!("Using file storage: ~/.smith/credentials.json");
                    try_file_set(&json)?;
                }
            }

            break;
        };

        println!(
            "No access token in response, trying again in {} seconds",
            device_auth_response.interval
        );

        std::thread::sleep(std::time::Duration::from_secs(
            device_auth_response.interval as u64,
        ));
    }

    Ok(())
}

pub fn logout() -> anyhow::Result<()> {
    // Try both storage methods
    let keyring_result = try_keyring_delete();
    let file_result = try_file_delete();

    // Success if either succeeded
    let keyring_ok = keyring_result.is_ok();
    let file_ok = file_result.is_ok();

    if keyring_ok || file_ok {
        println!("Logged out, credentials removed.");
        Ok(())
    } else {
        // Both failed with real errors
        Err(anyhow!("Failed to remove credentials"))
    }
}

pub async fn show(config: &Config) -> anyhow::Result<()> {
    let secrets = get_secrets(config).await?;
    let secrets = match secrets {
        Some(secrets) => secrets,
        None => {
            println!("Not logged in");
            return Ok(());
        }
    };

    let current_profile_secrets = secrets.profiles.get(&config.current_profile);

    match current_profile_secrets {
        Some(profile) => {
            println!("Profile: {}", config.current_profile);
            println!("\nAccess Token:");
            println!("{}", profile.access_token);
            println!("\nRefresh Token:");
            println!("{}", profile.refresh_token);

            let claims = decode_claims_without_verification(&profile.access_token)?;
            let expires_at = chrono::DateTime::from_timestamp(claims.exp, 0)
                .ok_or_else(|| anyhow!("Invalid timestamp"))?;
            let now = chrono::Utc::now();

            if claims.exp < now.timestamp() {
                println!(
                    "\nStatus: Expired at {}",
                    expires_at.format("%Y-%m-%d %H:%M:%S UTC")
                );
            } else {
                let duration = expires_at.signed_duration_since(now);
                println!(
                    "\nStatus: Valid for {} more minutes",
                    duration.num_minutes()
                );
                println!("Expires at: {}", expires_at.format("%Y-%m-%d %H:%M:%S UTC"));
            }
        }
        None => println!("Profile '{}' not found", config.current_profile),
    }

    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    exp: i64,
}

fn decode_claims_without_verification(token: &str) -> Result<Claims> {
    let parts: Vec<&str> = token.split('.').collect();

    if parts.len() != 3 {
        return Err(anyhow!("Token does not have 3 parts"));
    }

    let payload = parts[1];
    let decoded_payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload)?;
    let claims: Claims = serde_json::from_slice(&decoded_payload)?;

    Ok(claims)
}

fn is_token_expired(token: &str) -> bool {
    let claims = match decode_claims_without_verification(token) {
        Ok(claims) => claims,
        Err(_) => return true,
    };

    let now = chrono::Utc::now().timestamp();

    claims.exp < now
}

pub async fn refresh_access_token(
    domain: &str,
    client_id: &str,
    refresh_token: &str,
    audience: &str,
) -> Result<TokenResponse> {
    let client = Client::new();
    let token_endpoint = format!("https://{}/oauth/token", domain);

    let resp = client
        .post(token_endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
            ("audience", audience),
        ])
        .send()
        .await;

    match resp {
        Ok(response) => {
            if !response.status().is_success() {
                let error_text = response.text().await?;
                return Err(anyhow!("Token refresh failed: {}", error_text));
            }

            let token_response: TokenResponse = response.json().await?;
            Ok(token_response)
        }
        Err(e) => Err(anyhow!("Failed to refresh token: {}", e)),
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SessionSecrets {
    pub profiles: HashMap<String, ProfileSecrets>,
}

impl SessionSecrets {
    pub fn bearer_token(&self, profile_name: &str) -> Option<String> {
        self.profiles
            .get(profile_name)
            .map(|profile| profile.access_token.clone())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProfileSecrets {
    pub access_token: String,
    pub refresh_token: String,
}

pub async fn get_secrets(config: &Config) -> Result<Option<SessionSecrets>> {
    let current_profile = config.current_profile.clone();

    // Try keyring first, fall back to file storage
    let (secrets_json, storage_method) = match try_keyring_get() {
        Ok(Some(json)) => (json, StorageMethod::Keyring),
        Ok(None) => {
            // No keyring credentials, try file
            match try_file_get()? {
                Some(json) => (json, StorageMethod::File),
                None => return Ok(None), // Not logged in
            }
        }
        Err(e) if is_keyring_unavailable(&e) => {
            // Keyring unavailable, try file
            match try_file_get()? {
                Some(json) => (json, StorageMethod::File),
                None => return Ok(None),
            }
        }
        Err(e) => {
            // Real keyring error, warn and try file
            eprintln!("Warning: Keyring error: {}", e);
            match try_file_get()? {
                Some(json) => (json, StorageMethod::File),
                None => return Ok(None),
            }
        }
    };

    let mut session_secrets = serde_json::from_str::<SessionSecrets>(&secrets_json)?;

    // Check if the profile exists, return None if it doesn't
    if !session_secrets.profiles.contains_key(&current_profile) {
        return Ok(None);
    }

    let current_access_token = session_secrets
        .profiles
        .get(&current_profile)
        .unwrap() // Safe to unwrap because we checked above
        .access_token
        .clone();

    let current_refresh_token = session_secrets
        .profiles
        .get(&current_profile)
        .ok_or(anyhow!("Profile not found"))?
        .refresh_token
        .clone();

    if is_token_expired(&current_access_token) {
        let (domain, client_id, audience) = config.auth0_credentials();

        let token_response =
            refresh_access_token(&domain, &client_id, &current_refresh_token, &audience).await?;

        let new_access_token = token_response
            .access_token
            .ok_or(anyhow!("No access token in refresh response"))?;

        let new_refresh_token = token_response
            .refresh_token
            .unwrap_or_else(|| current_refresh_token.clone());
        let new_profile_secrets = ProfileSecrets {
            access_token: new_access_token,
            refresh_token: new_refresh_token,
        };

        if let Some(profile) = session_secrets.profiles.get_mut(&current_profile) {
            *profile = new_profile_secrets;
        } else {
            return Err(anyhow!("Profile not found"));
        }

        // Save refreshed tokens to same storage method
        let updated_json = serde_json::to_string(&session_secrets)?;
        match storage_method {
            StorageMethod::Keyring => try_keyring_set(&updated_json)?,
            StorageMethod::File => try_file_set(&updated_json)?,
        }

        return Ok(Some(session_secrets));
    }

    Ok(Some(session_secrets))
}

// Storage method tracking
#[derive(Debug, Clone, Copy)]
enum StorageMethod {
    Keyring,
    File,
}

// Get path to credentials file
fn credentials_file_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("Could not find home directory"))?;
    Ok(home.join(".smith").join("credentials.json"))
}

// Keyring operations
fn try_keyring_get() -> Result<Option<String>> {
    let user = whoami::username();
    let entry = Entry::new("SMITH_KEYS", &user)?;

    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(anyhow!("Keyring error: {}", e)),
    }
}

fn try_keyring_set(value: &str) -> Result<()> {
    let user = whoami::username();
    let entry = Entry::new("SMITH_KEYS", &user)?;
    entry.set_password(value)?;
    Ok(())
}

fn try_keyring_delete() -> Result<()> {
    let user = whoami::username();
    let entry = Entry::new("SMITH_KEYS", &user)?;
    entry.delete_credential()?;
    Ok(())
}

// File operations
fn try_file_get() -> Result<Option<String>> {
    let path = credentials_file_path()?;

    match File::open(&path) {
        Ok(mut file) => {
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;
            Ok(Some(contents))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow!("Failed to read credentials file: {}", e)),
    }
}

fn try_file_set(value: &str) -> Result<()> {
    let path = credentials_file_path()?;

    // Create directory if it doesn't exist
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Atomic write: write to temp file, then rename
    let temp_path = path.with_extension("tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temp_path)?;

    // Set permissions to 0600 (owner read/write only)
    let mut perms = file.metadata()?.permissions();
    perms.set_mode(0o600);
    file.set_permissions(perms)?;

    // Write content
    file.write_all(value.as_bytes())?;
    file.sync_all()?;
    drop(file);

    // Atomic rename
    std::fs::rename(&temp_path, &path)?;

    Ok(())
}

fn try_file_delete() -> Result<()> {
    let path = credentials_file_path()?;

    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(anyhow!("Failed to delete credentials file: {}", e)),
    }
}

// Utility to detect keyring unavailability
fn is_keyring_unavailable(error: &anyhow::Error) -> bool {
    let error_string = format!("{}", error);
    // Common error patterns for keyring unavailability
    error_string.contains("NoStorageAccess")
        || error_string.contains("org.freedesktop.secrets")
        || error_string.contains("DBus")
        || error_string.contains("Cannot autolaunch D-Bus")
        || error_string.contains("No such file or directory")
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use chrono::{Duration, Utc};

    // Test for decoding JWT claims
    #[test]
    fn test_decode_claims_without_verification() {
        let claims = Claims {
            exp: Utc::now().timestamp(),
        };

        let payload = serde_json::to_string(&claims).unwrap();
        let encoded_payload = URL_SAFE_NO_PAD.encode(payload);
        let token = format!("header.{}.signature", encoded_payload);

        let result = decode_claims_without_verification(&token);

        assert!(result.is_ok());
        assert_eq!(result.unwrap().exp, claims.exp);
    }

    #[test]
    fn test_decode_claims_invalid_token() {
        let result = decode_claims_without_verification("invalid.token.parts");
        assert!(result.is_err());
    }

    // Test for checking token expiration
    #[test]
    fn test_is_token_expired() {
        let claims = Claims {
            exp: (Utc::now() + Duration::seconds(60)).timestamp(),
        };

        let payload = serde_json::to_string(&claims).unwrap();
        let encoded_payload = URL_SAFE_NO_PAD.encode(payload);
        let token = format!("header.{}.signature", encoded_payload);

        assert!(!is_token_expired(&token));
    }

    #[test]
    fn test_is_token_expired_with_expired_token() {
        let claims = Claims {
            exp: (Utc::now() - Duration::seconds(60)).timestamp(),
        };

        let payload = serde_json::to_string(&claims).unwrap();
        let encoded_payload = URL_SAFE_NO_PAD.encode(payload);
        let token = format!("header.{}.signature", encoded_payload);

        assert!(is_token_expired(&token));
    }
}
