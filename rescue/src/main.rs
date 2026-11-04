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
use std::os::unix::net::UnixDatagram;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

const MAGIC_FILE: &str = "/etc/smith/magic.toml";
const BOOT_ID_FILE: &str = "/proc/sys/kernel/random/boot_id";
const POLL_INTERVAL: Duration = Duration::from_secs(60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
/// Shorter than the unit's `WatchdogSec`.
const WATCHDOG_EVERY: Duration = Duration::from_secs(30);

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

struct RealDevice;

impl reboot::Device for RealDevice {
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
                    if let Err(err) = child.wait() {
                        warn!("Could not reap {program}: {err}");
                    }
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

    fn sync(&mut self) {
        nix::unistd::sync();
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
}
