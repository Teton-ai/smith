use anyhow::{Context, Result, anyhow};
use fs2::FileExt;
use nix::unistd::{Gid, Uid, User, chown};
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use tokio::process::Command;
use tokio::task;
use tracing::{debug, error, info, warn};

pub async fn ensure_ssh_dir(login: &str) -> Result<(PathBuf, u32, u32)> {
    let login = login.to_owned();

    let (ssh, uid, gid) = task::spawn_blocking(move || -> Result<(PathBuf, u32, u32)> {
        use std::fs;

        let entry = User::from_name(&login)
            .with_context(|| format!("looking up user {login}"))?
            .ok_or_else(|| anyhow!("user {login} not found"))?;

        let uid = entry.uid.as_raw();
        let gid = entry.gid.as_raw();

        let ssh = entry.dir.join(".ssh");

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
            .truncate(false)
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
            .truncate(false)
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
            if l.split_whitespace()
                .last()
                .is_none_or(|last_part| last_part != tag)
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

/// sshd drop-in that disables every password-style SSH login, leaving only
/// key-based auth. Kept in the dedicated drop-in dir (not the main config) so
/// it survives openssh-server upgrades and is trivially attributable/removable.
const SSH_HARDEN_DROPIN: &str = "/etc/ssh/sshd_config.d/10-smith-harden.conf";
const SSH_HARDEN_BODY: &str = "# Managed by smithd - do not edit.\n\
# Disables shared-password SSH logins; key-based auth only.\n\
PasswordAuthentication no\n\
KbdInteractiveAuthentication no\n\
ChallengeResponseAuthentication no\n";
/// Marker prepended to directives we comment out in the fallback path, so the
/// edit is attributable and our own re-runs stay idempotent.
const SSH_HARDEN_MARKER: &str = "# disabled by smithd";

/// Disable password-based SSH authentication on this device (key-only).
///
/// Safe to call on every boot: it only reloads sshd when it actually changed
/// something, validates via `sshd -T` before reloading, and refuses to act if
/// public-key auth isn't available (which would otherwise lock everyone out).
pub async fn disable_ssh_password_auth() -> Result<()> {
    let mut changed = write_ssh_harden_dropin().await?;

    // The effective config (after all includes) is the only real source of
    // truth — drop-in precedence depends on file ordering we don't control.
    let mut effective = sshd_effective_config().await?;

    // Something earlier in the include order still permits passwords: neutralize
    // the conflicting directives so our drop-in becomes authoritative.
    if effective.get("passwordauthentication").map(String::as_str) == Some("yes") {
        warn!(
            "sshd still permits password auth after drop-in; neutralizing conflicting directives"
        );
        if neutralize_ssh_conflicts().await? {
            changed = true;
            effective = sshd_effective_config().await?;
        }
    }

    // Never trade a shared password for a total lockout: if key auth isn't on,
    // leave password auth as-is and shout about it instead.
    if effective.get("pubkeyauthentication").map(String::as_str) != Some("yes") {
        error!(
            "Refusing to disable SSH passwords: PubkeyAuthentication is not enabled; \
             doing so would lock out all access to this device"
        );
        return Ok(());
    }

    if effective.get("passwordauthentication").map(String::as_str) != Some("no") {
        error!("Could not disable SSH password authentication; effective config still permits it");
        return Ok(());
    }

    if changed {
        reload_sshd().await?;
        info!("Disabled SSH password authentication and reloaded sshd");
    } else {
        info!("SSH password authentication already disabled");
    }

    Ok(())
}

/// Write the hardening drop-in atomically. Returns whether the on-disk content
/// changed (so callers can skip an unnecessary sshd reload).
async fn write_ssh_harden_dropin() -> Result<bool> {
    let path = Path::new(SSH_HARDEN_DROPIN);

    if let Ok(existing) = tokio::fs::read_to_string(path).await
        && existing == SSH_HARDEN_BODY
    {
        return Ok(false);
    }

    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir)
            .await
            .with_context(|| format!("creating {dir:?}"))?;
    }

    write_file_atomic(path, SSH_HARDEN_BODY, 0o644).await?;
    Ok(true)
}

/// Parse `sshd -T` into a map of effective directive -> value (both lowercased).
async fn sshd_effective_config() -> Result<HashMap<String, String>> {
    let output = Command::new("sshd")
        .arg("-T")
        .output()
        .await
        .context("running `sshd -T`")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("`sshd -T` failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut config = HashMap::new();
    for line in stdout.lines() {
        if let Some((key, value)) = line.split_once(' ') {
            config.insert(key.to_ascii_lowercase(), value.trim().to_ascii_lowercase());
        }
    }
    Ok(config)
}

/// Fallback: comment out password-auth directives in the main config and any
/// other drop-ins so our drop-in wins regardless of include ordering. Returns
/// whether any file was modified.
async fn neutralize_ssh_conflicts() -> Result<bool> {
    const KEYWORDS: [&str; 3] = [
        "passwordauthentication",
        "kbdinteractiveauthentication",
        "challengeresponseauthentication",
    ];

    let mut files = vec![PathBuf::from("/etc/ssh/sshd_config")];
    if let Ok(mut entries) = tokio::fs::read_dir("/etc/ssh/sshd_config.d").await {
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("conf")
                && path != Path::new(SSH_HARDEN_DROPIN)
            {
                files.push(path);
            }
        }
    }

    let mut changed = false;
    for file in files {
        changed |= comment_ssh_directives(&file, &KEYWORDS).await?;
    }
    Ok(changed)
}

/// Comment out any active line whose first token is one of `keywords`.
async fn comment_ssh_directives(path: &Path, keywords: &[&str]) -> Result<bool> {
    let content = match tokio::fs::read_to_string(path).await {
        Ok(content) => content,
        Err(_) => return Ok(false),
    };

    let mut changed = false;
    let mut out = String::with_capacity(content.len());
    for line in content.lines() {
        let trimmed = line.trim_start();
        let is_active_directive = !trimmed.starts_with('#')
            && trimmed
                .split_whitespace()
                .next()
                .is_some_and(|kw| keywords.contains(&kw.to_ascii_lowercase().as_str()));

        if is_active_directive {
            out.push_str(SSH_HARDEN_MARKER);
            out.push(' ');
            changed = true;
        }
        out.push_str(line);
        out.push('\n');
    }

    if changed {
        write_file_atomic(path, &out, 0o644).await?;
    }
    Ok(changed)
}

/// Reload sshd, tolerating the Debian (`ssh`) vs other (`sshd`) unit naming.
async fn reload_sshd() -> Result<()> {
    for service in ["ssh", "sshd"] {
        match Command::new("systemctl")
            .arg("reload")
            .arg(service)
            .output()
            .await
        {
            Ok(output) if output.status.success() => return Ok(()),
            Ok(output) => debug!(
                "`systemctl reload {service}` failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            Err(err) => debug!("could not run `systemctl reload {service}`: {err}"),
        }
    }
    Err(anyhow!(
        "failed to reload sshd via systemctl (tried ssh and sshd)"
    ))
}

/// Atomically replace `path` with `contents` at the given mode, via a temp file
/// in the same directory (same pattern as the authorized_keys writers above).
async fn write_file_atomic(path: &Path, contents: &str, mode: u32) -> Result<()> {
    let path = path.to_owned();
    let contents = contents.to_owned();
    task::spawn_blocking(move || -> Result<()> {
        use std::io::Write;

        let dir = path
            .parent()
            .ok_or_else(|| anyhow!("path {path:?} has no parent directory"))?;
        let mut tmp = NamedTempFile::new_in(dir)?;
        tmp.write_all(contents.as_bytes())?;
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(mode))?;
        tmp.persist(&path)?;
        Ok(())
    })
    .await?
}
