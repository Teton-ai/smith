use anyhow::{Result, anyhow};
use keyring::Entry;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

// Credential storage abstraction
pub trait CredentialStore {
    fn get(&self) -> Result<String>;
    fn set(&self, value: &str) -> Result<()>;
    fn delete(&self) -> Result<()>;
}

// File-based storage (plain JSON with 0600 permissions)
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    fn new() -> Result<Self> {
        let smith_dir = dirs::home_dir()
            .ok_or_else(|| anyhow!("Cannot determine home directory"))?
            .join(".smith");

        fs::create_dir_all(&smith_dir)?;

        let path = smith_dir.join("credentials.json");

        Ok(Self { path })
    }

    fn file_exists(&self) -> bool {
        self.path.exists()
    }
}

impl CredentialStore for FileStore {
    fn get(&self) -> Result<String> {
        let contents = fs::read_to_string(&self.path)?;
        Ok(contents)
    }

    fn set(&self, value: &str) -> Result<()> {
        fs::write(&self.path, value)?;

        // Set file permissions to 0600 (owner read/write only)
        let mut perms = fs::metadata(&self.path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&self.path, perms)?;

        Ok(())
    }

    fn delete(&self) -> Result<()> {
        if self.path.exists() {
            fs::remove_file(&self.path)?;
        }
        Ok(())
    }
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

// Check if file-based storage is allowed (Docker/container mode)
fn is_file_storage_allowed() -> bool {
    std::env::var("SMITH_USE_FILE_STORE").is_ok()
}

// Factory function to get the appropriate credential store
pub fn get_credential_store(_profile: &str) -> Result<Box<dyn CredentialStore>> {
    // Prefer keychain first (most secure)
    let keychain = KeychainStore::new();

    // If file storage is allowed, check if file exists and prefer it
    // (this handles Docker case where keychain won't work but file is mounted)
    if is_file_storage_allowed() {
        let file_store = FileStore::new()?;
        if file_store.file_exists() {
            return Ok(Box::new(file_store));
        }
        // File doesn't exist yet - return keychain to try first,
        // caller will handle fallback to file on keychain failure
        return Ok(Box::new(keychain));
    }

    // Default: return keychain (will error during actual use if unavailable)
    Ok(Box::new(keychain))
}
