use anyhow::{Context, Result, anyhow};
use fs2::FileExt;
use nix::unistd::{Gid, Uid, User, chown};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tempfile::NamedTempFile;
use tokio::task;
use tracing::info;

pub async fn ensure_ssh_dir(login: &str) -> Result<(PathBuf, u32, u32)> {
    let login = login.to_owned();

    let (ssh, uid, gid) = task::spawn_blocking(move || -> Result<(PathBuf, u32, u32)> {
        use std::fs;

        let entry = User::from_name(&login)
            .with_context(|| format!("looking up user {login}"))?
            .ok_or_else(|| anyhow!("user {login} not found"))?;

        let uid = entry.uid.as_raw();
        let gid = entry.gid.as_raw();

        let ssh = PathBuf::from(entry.dir).join(".ssh");

        let canonical_home = ssh
            .parent()
            .ok_or_else(|| anyhow!("home has no parent"))?
            .canonicalize()?;
        if !canonical_home.starts_with("/home") {
            anyhow::bail!("refusing to touch {:?}", canonical_home);
        }

        if !ssh.exists() {
            fs::create_dir_all(&ssh)?;
        }
        fs::set_permissions(&ssh, fs::Permissions::from_mode(0o700))?;
        nix::unistd::chown(&ssh, Some(Uid::from_raw(uid)), Some(Gid::from_raw(gid)))?;

        Ok((ssh, uid, gid))
    })
    .await??;

    Ok((ssh, uid, gid))
}

pub async fn add_key(user: &str, pubkey: &str, tag: String) -> Result<()> {
    // Validate SSH public key format
    if !pubkey.starts_with("ssh-rsa ") && !pubkey.starts_with("ssh-ed25519 ") {
        return Err(anyhow!("Invalid SSH public key format"));
    }

    let pubkey = pubkey.to_owned();
    let (ssh_folder, uid, gid) = ensure_ssh_dir(user).await?;
    let mut auth_keys = ssh_folder.clone();
    auth_keys.push("authorized_keys");
    let mut lock_file = ssh_folder.clone();
    lock_file.push("authorized_keys.lock");

    task::spawn_blocking(move || {
        use std::{
            fs::{File, OpenOptions},
            io::{BufRead, BufReader, Write},
        };

        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .open(lock_file)?;
        lock.lock_exclusive()?; // blocks only worker thread

        // collect current lines minus stale tag
        let mut lines = Vec::<String>::new();
        if let Ok(file) = File::open(&auth_keys) {
            for l in BufReader::new(file).lines() {
                let l = l?;
                // Skip if this key already exists
                if l.starts_with(&pubkey) {
                    return Ok(());
                }
                lines.push(l);
            }
        }

        lines.push(format!("{pubkey} {tag}"));

        let mut tmp = NamedTempFile::new_in(ssh_folder)?;
        for l in &lines {
            writeln!(tmp, "{l}")?;
        }
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
        chown(tmp.path(), Some(uid.into()), Some(gid.into()))?;
        tmp.persist(auth_keys)?;

        Ok::<_, anyhow::Error>(())
    })
    .await?
}

pub async fn remove_key(user: &str, tag: &str) -> Result<()> {
    info!("Removing key for user {user} with tag {tag}");
    let (ssh_folder, _uid, _gid) = ensure_ssh_dir(user).await?;
    let mut auth_keys = ssh_folder.clone();
    auth_keys.push("authorized_keys");
    let mut lock_file = ssh_folder.clone();
    lock_file.push("authorized_keys.lock");
    let tag = tag.to_owned();
    task::spawn_blocking(move || {
        use std::{
            fs::{File, OpenOptions},
            io::{BufRead, BufReader, Write},
        };

        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .open(lock_file)?;
        lock.lock_exclusive()?;

        let file = match File::open(&auth_keys) {
            Ok(f) => f,
            Err(_) => return Ok(()), // nothing to do
        };

        let mut lines = Vec::<String>::new();
        for l in BufReader::new(file).lines() {
            let l = l?;
            if !l
                .split_whitespace()
                .last()
                .map_or(false, |last_part| last_part == tag)
            {
                lines.push(l);
            }
        }

        let mut tmp = NamedTempFile::new_in(ssh_folder)?;
        for l in &lines {
            writeln!(tmp, "{l}")?;
        }
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
        tmp.persist(auth_keys)?;

        Ok::<_, anyhow::Error>(())
    })
    .await?
}
