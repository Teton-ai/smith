use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use anyhow::{Result, anyhow};
use keyring::Entry;
use std::fs;
use std::path::PathBuf;

// Credential storage abstraction
pub trait CredentialStore {
    fn get(&self) -> Result<String>;
    fn set(&self, value: &str) -> Result<()>;
    fn delete(&self) -> Result<()>;
}

// Keychain-based storage (existing behavior)
pub struct KeychainStore {
    user: String,
}

impl KeychainStore {
    fn new() -> Self {
        Self {
            user: whoami::username(),
        }
    }
}

impl CredentialStore for KeychainStore {
    fn get(&self) -> Result<String> {
        let entry = Entry::new("SMITH_KEYS", &self.user)?;
        let password = entry.get_password()?;
        Ok(password)
    }

    fn set(&self, value: &str) -> Result<()> {
        let entry = Entry::new("SMITH_KEYS", &self.user)?;
        entry.set_password(value)?;
        Ok(())
    }

    fn delete(&self) -> Result<()> {
        let entry = Entry::new("SMITH_KEYS", &self.user)?;
        entry.delete_credential()?;
        Ok(())
    }
}

// File-based storage with encryption
pub struct FileStore {
    path: PathBuf,
    cipher: Aes256Gcm,
}

impl FileStore {
    fn new() -> Result<Self> {
        let smith_dir = dirs::home_dir()
            .ok_or_else(|| anyhow!("Cannot determine home directory"))?
            .join(".smith");

        fs::create_dir_all(&smith_dir)?;

        let path = smith_dir.join("credentials.json");

        // Derive encryption key from machine UID
        let machine_id =
            machine_uid::get().map_err(|e| anyhow!("Failed to get machine ID: {}", e))?;

        // Create a 32-byte key from the machine ID
        let mut key_bytes = [0u8; 32];
        let machine_id_bytes = machine_id.as_bytes();
        for (i, byte) in machine_id_bytes.iter().enumerate() {
            key_bytes[i % 32] ^= byte;
        }

        let cipher = Aes256Gcm::new_from_slice(&key_bytes)
            .map_err(|e| anyhow!("Failed to create cipher: {}", e))?;

        Ok(Self { path, cipher })
    }

    fn encrypt(&self, plaintext: &str) -> Result<Vec<u8>> {
        // Use a fixed nonce (this is acceptable for our use case as we're using machine-specific key)
        let nonce = Nonce::from_slice(b"smithcreds00"); // 12 bytes

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| anyhow!("Encryption failed: {}", e))?;

        Ok(ciphertext)
    }

    fn decrypt(&self, ciphertext: &[u8]) -> Result<String> {
        let nonce = Nonce::from_slice(b"smithcreds00");

        let plaintext = self
            .cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow!("Decryption failed: {}", e))?;

        String::from_utf8(plaintext).map_err(|e| anyhow!("Invalid UTF-8: {}", e))
    }
}

impl CredentialStore for FileStore {
    fn get(&self) -> Result<String> {
        let encrypted_data = fs::read(&self.path)?;
        self.decrypt(&encrypted_data)
    }

    fn set(&self, value: &str) -> Result<()> {
        let encrypted_data = self.encrypt(value)?;
        fs::write(&self.path, encrypted_data)?;
        Ok(())
    }

    fn delete(&self) -> Result<()> {
        if self.path.exists() {
            fs::remove_file(&self.path)?;
        }
        Ok(())
    }
}

// Factory function to get the appropriate credential store
pub fn get_credential_store() -> Box<dyn CredentialStore> {
    // Try keychain first
    let keychain = KeychainStore::new();
    if keychain.get().is_ok() || test_keychain_write(&keychain) {
        return Box::new(keychain);
    }

    // Fall back to file storage
    match FileStore::new() {
        Ok(file_store) => Box::new(file_store),
        Err(_) => Box::new(keychain), // If file store fails, return keychain anyway (will error later)
    }
}

// Test if keychain is available by trying to write and delete a test entry
fn test_keychain_write(store: &KeychainStore) -> bool {
    let test_value = "test";
    store.set(test_value).is_ok() && store.delete().is_ok()
}
