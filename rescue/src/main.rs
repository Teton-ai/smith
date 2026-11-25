//! smith-rescue: the last way to recover a device that has internet but whose
//! smithd is stuck. It polls the api's restart channel each minute and reboots
//! when a Restart command waits for this boot.
//!
//! It is one thread with no async runtime, and each call has a timeout, so a
//! loop step always ends. It reads smithd's magic.toml but shares nothing else
//! with smithd.

mod reboot;

use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

const MAGIC_FILE: &str = "/etc/smith/magic.toml";
const BOOT_ID_FILE: &str = "/proc/sys/kernel/random/boot_id";
const POLL_INTERVAL: Duration = Duration::from_secs(60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
/// Shorter than the unit's `WatchdogSec`.
const WATCHDOG_EVERY: Duration = Duration::from_secs(30);
const SYNC_TIMEOUT: Duration = Duration::from_secs(30);
/// Locked in the same order as apt and dpkg: frontend first, then dpkg itself.
const DPKG_LOCK_FILES: [&str; 2] = ["/var/lib/dpkg/lock-frontend", "/var/lib/dpkg/lock"];
const DPKG_LOCK_TIMEOUT: Duration = Duration::from_secs(180);
const DPKG_LOCK_POLL: Duration = Duration::from_secs(1);

#[derive(Deserialize)]
struct Magic {
    meta: Meta,
}

#[derive(Deserialize)]
struct Meta {
    server: String,
    token: Option<String>,
}

#[derive(Deserialize)]
struct PollResponse {
    target_boot: Option<String>,
}

fn main() {
    // It takes no arguments. Any argument, such as `--help`, must not start
    // the loop, because the loop can reboot the device.
    if std::env::args_os().len() > 1 {
        eprintln!(
            "smith-rescue {}: takes no arguments; systemd runs it as smith-rescue.service",
            env!("CARGO_PKG_VERSION")
        );
        std::process::exit(2);
    }

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    info!("smith-rescue {} running", env!("CARGO_PKG_VERSION"));

    let client = match reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .connect_timeout(HTTP_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            // systemd starts the process again.
            error!("Could not build the HTTP client: {err:#}");
            std::process::exit(1);
        }
    };

    let mut device = RealDevice;
    loop {
        feed_watchdog();
        match must_reboot(&client) {
            Ok(true) => {
                info!("A Restart waits for this boot; rebooting");
                reboot::reboot(&mut device);
            }
            Ok(false) => {}
            Err(err) => warn!("Restart poll failed: {err:#}"),
        }
        reboot::Device::wait(&mut device, POLL_INTERVAL);
    }
}

/// Polls the restart channel. True when the api asks to end this boot.
fn must_reboot(client: &reqwest::blocking::Client) -> Result<bool> {
    let boot_id = read_boot_id()?;
    let magic =
        std::fs::read_to_string(MAGIC_FILE).with_context(|| format!("reading {MAGIC_FILE}"))?;
    let (server, token) = parse_magic(&magic)?;
    let Some(token) = token else {
        // smithd has not registered the device yet.
        return Ok(false);
    };

    let response = client
        .get(format!("{}/restart", server.trim_end_matches('/')))
        .query(&[("boot_id", &boot_id)])
        .bearer_auth(token)
        .send()
        .context("sending the poll")?
        .error_for_status()
        .context("the api refused the poll")?
        .json::<PollResponse>()
        .context("reading the answer")?;
    Ok(is_this_boot(response.target_boot.as_deref(), &boot_id))
}

fn read_boot_id() -> Result<String> {
    let boot_id =
        std::fs::read_to_string(BOOT_ID_FILE).with_context(|| format!("reading {BOOT_ID_FILE}"))?;
    Ok(boot_id.trim().to_ascii_lowercase())
}

fn parse_magic(contents: &str) -> Result<(String, Option<String>)> {
    let magic: Magic = toml::from_str(contents).context("parsing magic.toml")?;
    let token = magic.meta.token.filter(|token| !token.is_empty());
    Ok((magic.meta.server, token))
}

/// A Restart for another boot is one that a reboot already ended.
fn is_this_boot(target_boot: Option<&str>, boot_id: &str) -> bool {
    target_boot.is_some_and(|target| target.trim().eq_ignore_ascii_case(boot_id))
}

/// Tells systemd the loop is alive. No-op outside systemd.
fn feed_watchdog() {
    let Some(socket) = std::env::var_os("NOTIFY_SOCKET") else {
        return;
    };
    if let Err(err) = notify_systemd(&socket, "WATCHDOG=1") {
        warn!("Could not feed the systemd watchdog: {err}");
    }
}

/// `socket` is a path, or an abstract name when it starts with `@`.
fn notify_systemd(socket: &std::ffi::OsStr, state: &str) -> std::io::Result<()> {
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::net::SocketAddr;

    let addr = match socket.as_bytes().strip_prefix(b"@") {
        Some(name) => SocketAddr::from_abstract_name(name)?,
        None => SocketAddr::from_pathname(socket)?,
    };
    UnixDatagram::unbound()?.send_to_addr(state.as_bytes(), &addr)?;
    Ok(())
}

/// Keeps acquired dpkg lock files open until the reboot finishes or returns.
struct DpkgLockGuard(#[allow(dead_code)] Vec<File>);

fn lock_file_nonblocking(file: &File) -> std::result::Result<(), nix::errno::Errno> {
    let fl = nix::libc::flock {
        l_type: nix::libc::F_WRLCK as _,
        l_whence: nix::libc::SEEK_SET as _,
        l_start: 0,
        l_len: 0,
        l_pid: 0,
    };
    nix::fcntl::fcntl(file, nix::fcntl::FcntlArg::F_SETLK(&fl))?;
    Ok(())
}

/// Tries to lock every existing dpkg lock file without blocking. Returns `None`
/// (and releases any lock taken earlier in this call) when dpkg or apt holds one.
fn try_acquire_dpkg_locks(paths: &[PathBuf]) -> Option<Vec<File>> {
    let mut held = Vec::with_capacity(paths.len());
    for path in paths {
        let file = match OpenOptions::new().write(true).open(path) {
            Ok(file) => file,
            Err(err) if err.kind() == ErrorKind::NotFound => continue,
            Err(err) => {
                warn!("Could not open {}: {err}; skipping lock", path.display());
                continue;
            }
        };
        match lock_file_nonblocking(&file) {
            Ok(()) => held.push(file),
            Err(nix::errno::Errno::EAGAIN | nix::errno::Errno::EACCES) => {
                return None;
            }
            Err(err) => {
                warn!("Could not lock {}: {err}; skipping lock", path.display());
            }
        }
    }
    Some(held)
}

/// Waits up to `timeout` for dpkg/apt to finish, then holds the locks so no new
/// package install starts before power cuts. Runs on a helper thread so a stuck
/// disk cannot block the reboot past `timeout`.
fn acquire_dpkg_locks(
    paths: &[PathBuf],
    timeout: Duration,
    poll_interval: Duration,
) -> DpkgLockGuard {
    let paths = paths.to_vec();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_worker = Arc::clone(&stop);
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut logged_wait = false;
        loop {
            if stop_worker.load(Ordering::Relaxed) {
                return;
            }
            if let Some(files) = try_acquire_dpkg_locks(&paths) {
                if logged_wait {
                    info!("dpkg lock cleared; continuing with the reboot");
                }
                // After the timeout nobody waits for this; dropping `files`
                // releases the locks.
                if done.send(files).is_err() {
                    info!("dpkg lock cleared after the reboot went on");
                }
                return;
            }
            if !logged_wait {
                info!("Waiting up to {timeout:?} for dpkg to finish before rebooting");
                logged_wait = true;
            }
            std::thread::sleep(poll_interval);
        }
    });

    let deadline = Instant::now() + timeout;
    loop {
        feed_watchdog();
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match finished.recv_timeout(left.min(Duration::from_secs(1))) {
            Ok(files) => return DpkgLockGuard(files),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return DpkgLockGuard(Vec::new());
            }
        }
    }

    stop.store(true, Ordering::Relaxed);
    warn!("dpkg lock still held after {timeout:?}; going on with the reboot");
    DpkgLockGuard(Vec::new())
}

struct RealDevice;

impl reboot::Device for RealDevice {
    type LockGuard = DpkgLockGuard;

    fn lock_dpkg(&mut self) -> Self::LockGuard {
        let paths: Vec<PathBuf> = DPKG_LOCK_FILES.iter().map(PathBuf::from).collect();
        acquire_dpkg_locks(&paths, DPKG_LOCK_TIMEOUT, DPKG_LOCK_POLL)
    }

    fn has_program(&self, program: &str) -> bool {
        std::env::var_os("PATH")
            .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
    }

    fn run(&mut self, program: &str, args: &[&str], timeout: Duration) -> bool {
        let mut child = match Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(err) => {
                warn!("Could not start {program}: {err}");
                return false;
            }
        };
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return status.success(),
                Ok(None) if Instant::now() >= deadline => {
                    warn!("{program} did not finish in {timeout:?}; killing it");
                    if let Err(err) = child.kill() {
                        warn!("Could not kill {program}: {err}");
                    }
                    // A child stuck in disk I/O ignores SIGKILL, so it is
                    // reaped on another thread and the reboot goes on.
                    let program = program.to_owned();
                    std::thread::spawn(move || {
                        if let Err(err) = child.wait() {
                            warn!("Could not reap {program}: {err}");
                        }
                    });
                    return false;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(200)),
                Err(err) => {
                    warn!("Could not wait for {program}: {err}");
                    return false;
                }
            }
        }
    }

    fn wait(&mut self, duration: Duration) {
        let deadline = Instant::now() + duration;
        loop {
            feed_watchdog();
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            std::thread::sleep(left.min(WATCHDOG_EVERY));
        }
    }

    /// A stuck disk can block `sync` for ever. It runs on its own thread, and
    /// the reboot goes on after `SYNC_TIMEOUT` even when it has not returned.
    fn sync(&mut self) {
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            nix::unistd::sync();
            // After the timeout nobody waits for this; a failed send is expected.
            if done.send(()).is_err() {
                info!("sync returned after the reboot went on");
            }
        });
        let deadline = Instant::now() + SYNC_TIMEOUT;
        while Instant::now() < deadline {
            feed_watchdog();
            if finished.recv_timeout(Duration::from_secs(1)).is_ok() {
                return;
            }
        }
        warn!("sync did not return in {SYNC_TIMEOUT:?}; going on with the reboot");
    }

    fn force_reboot(&mut self) {
        let Err(err) = nix::sys::reboot::reboot(nix::sys::reboot::RebootMode::RB_AUTOBOOT);
        error!("reboot(2) failed: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_and_token_come_from_magic_toml() -> Result<()> {
        let magic = r#"
            [meta]
            magic_version = 2
            server = "https://api.smith.teton.ai/smith"
            token = "abc"

            [tunnel]
            server = "bore"
            secret = ""
        "#;
        assert_eq!(
            parse_magic(magic)?,
            (
                "https://api.smith.teton.ai/smith".into(),
                Some("abc".into())
            )
        );
        Ok(())
    }

    #[test]
    fn an_unregistered_device_has_no_token() -> Result<()> {
        let magic = "[meta]\nmagic_version = 2\nserver = \"http://api:8080/smith\"\ntoken = \"\"\n";
        assert_eq!(parse_magic(magic)?.1, None);
        Ok(())
    }

    #[test]
    fn only_a_restart_for_this_boot_reboots() {
        let boot = "6f1c1d4e-0b55-4a4b-9a43-3c1e6d2b9f00";
        assert!(is_this_boot(Some(boot), boot));
        assert!(is_this_boot(Some(&boot.to_ascii_uppercase()), boot));
        assert!(!is_this_boot(
            Some("1b8e4d1a-77f0-4f64-a0d4-6a8c2f2d1e11"),
            boot
        ));
        assert!(!is_this_boot(None, boot));
    }

    /// Takes an open-file-description write lock. On Linux this conflicts with
    /// `F_SETLK` even inside the same process, letting tests simulate dpkg.
    fn try_ofd_lock(file: &File) -> std::result::Result<(), nix::errno::Errno> {
        let fl = nix::libc::flock {
            l_type: nix::libc::F_WRLCK as _,
            l_whence: nix::libc::SEEK_SET as _,
            l_start: 0,
            l_len: 0,
            l_pid: 0,
        };
        nix::fcntl::fcntl(file, nix::fcntl::FcntlArg::F_OFD_SETLK(&fl))?;
        Ok(())
    }

    fn temp_lock_dir(name: &str) -> Result<PathBuf> {
        let dir =
            std::env::temp_dir().join(format!("smith-rescue-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    #[test]
    fn dpkg_lock_waits_for_holder_and_keeps_lock_until_dropped() -> Result<()> {
        let dir = temp_lock_dir("wait-and-hold")?;
        let frontend = dir.join("lock-frontend");
        let lock = dir.join("lock");
        File::create(&frontend)?;
        File::create(&lock)?;

        let holder = OpenOptions::new().write(true).open(&lock)?;
        try_ofd_lock(&holder)?;

        let release_after = Duration::from_millis(80);
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(release_after);
            drop(holder);
        });

        let start = Instant::now();
        let guard = acquire_dpkg_locks(
            &[frontend.clone(), lock.clone()],
            Duration::from_secs(2),
            Duration::from_millis(20),
        );
        assert!(
            start.elapsed() >= release_after,
            "acquired before holder released"
        );
        assert_eq!(guard.0.len(), 2);

        // While the guard lives, another process (simulated by OFD lock) cannot lock either file.
        let probe_frontend = OpenOptions::new().write(true).open(&frontend)?;
        let probe_lock = OpenOptions::new().write(true).open(&lock)?;
        assert_eq!(
            try_ofd_lock(&probe_frontend),
            Err(nix::errno::Errno::EAGAIN)
        );
        assert_eq!(try_ofd_lock(&probe_lock), Err(nix::errno::Errno::EAGAIN));

        drop(guard);
        assert!(try_ofd_lock(&probe_frontend).is_ok());
        assert!(try_ofd_lock(&probe_lock).is_ok());

        assert!(releaser.join().is_ok());
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn dpkg_lock_proceeds_when_timeout_expires() -> Result<()> {
        let dir = temp_lock_dir("timeout")?;
        let lock = dir.join("lock-frontend");
        File::create(&lock)?;

        let holder = OpenOptions::new().write(true).open(&lock)?;
        try_ofd_lock(&holder)?;

        let timeout = Duration::from_millis(80);
        let start = Instant::now();
        let guard = acquire_dpkg_locks(&[lock], timeout, Duration::from_millis(20));
        assert!(start.elapsed() >= timeout);
        assert!(guard.0.is_empty());

        drop(holder);
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn missing_dpkg_lock_files_are_skipped() -> Result<()> {
        let dir = temp_lock_dir("missing")?;
        let missing = dir.join("does-not-exist");
        let guard = acquire_dpkg_locks(
            &[missing],
            Duration::from_secs(1),
            Duration::from_millis(20),
        );
        assert!(guard.0.is_empty());
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
