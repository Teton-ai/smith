use crate::commander::CommanderHandle;
use crate::shutdown::ShutdownSignals;
use crate::utils::schema::{SafeCommandResponse, SafeCommandRx};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};
use tracing::{error, info, warn};

/// Run a full audit every 12 hours.
const AUDIT_INTERVAL_SECS: u64 = 12 * 60 * 60;

/// Synthetic command id for autonomously-reported audits (daemon start and the
/// periodic timer), following the negative-id convention the postman uses for
/// unsolicited device state.
const AUDIT_RESULT_ID: i32 = -5;

pub enum ActorMessage {
    RunAudit,
}

/// Auditor Actor: periodically (and on demand) runs host compliance checks and
/// stages the result on the commander, which reports it to the API on the next
/// poll — the same channel the device uses for system info.
pub struct Actor {
    shutdown: ShutdownSignals,
    receiver: mpsc::Receiver<ActorMessage>,
    commander: CommanderHandle,
}

impl Actor {
    pub fn new(
        shutdown: ShutdownSignals,
        receiver: mpsc::Receiver<ActorMessage>,
        commander: CommanderHandle,
    ) -> Self {
        Self {
            shutdown,
            receiver,
            commander,
        }
    }

    async fn handle_message(&mut self, msg: ActorMessage) {
        match msg {
            ActorMessage::RunAudit => self.run_audit().await,
        }
    }

    async fn run_audit(&self) {
        let (disk_encrypted, password_access_disabled) = run_audit_checks().await;
        self.commander
            .insert_result(vec![SafeCommandResponse {
                id: AUDIT_RESULT_ID,
                command: SafeCommandRx::AuditReport {
                    disk_encrypted,
                    password_access_disabled,
                },
                status: 0,
            }])
            .await;
    }

    pub async fn run(&mut self) {
        info!("Auditor starting");

        // First tick fires after a full interval; the daemon-start audit arrives
        // as an explicit `RunAudit` message instead, so we don't double-audit on
        // boot.
        let start = time::Instant::now() + Duration::from_secs(AUDIT_INTERVAL_SECS);
        let mut audit_interval = time::interval_at(start, Duration::from_secs(AUDIT_INTERVAL_SECS));

        loop {
            tokio::select! {
                Some(msg) = self.receiver.recv() => {
                    self.handle_message(msg).await;
                }
                _ = audit_interval.tick() => {
                    self.run_audit().await;
                }
                _ = self.shutdown.token.cancelled() => {
                    break;
                }
            }
        }
        info!("Auditor shutting down");
    }
}

/// Run the host compliance checks, returning `(disk_encrypted,
/// password_access_disabled)`. Each value is `None` when its probe couldn't be
/// run or parsed.
pub async fn run_audit_checks() -> (Option<bool>, Option<bool>) {
    let disk_encrypted = check_disk_encrypted().await;
    let password_access_disabled = check_password_auth_disabled().await;

    info!(?disk_encrypted, ?password_access_disabled, "Audit complete");

    (disk_encrypted, password_access_disabled)
}

/// True if any block device is a LUKS/crypt mapping. `None` if lsblk can't be
/// run or its output can't be parsed.
async fn check_disk_encrypted() -> Option<bool> {
    let output = Command::new("lsblk")
        .args(["-J", "-o", "TYPE"])
        .output()
        .await
        .inspect_err(|e| error!("Failed to run lsblk: {e}"))
        .ok()?;

    if !output.status.success() {
        warn!("lsblk exited with status {:?}", output.status);
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .inspect_err(|e| error!("Failed to parse lsblk JSON: {e}"))
        .ok()?;

    Some(json_has_crypt(&json))
}

/// Recursively scans lsblk JSON for any node whose `type` is `crypt`.
fn json_has_crypt(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            if map.get("type").and_then(|t| t.as_str()) == Some("crypt") {
                return true;
            }
            map.values().any(json_has_crypt)
        }
        serde_json::Value::Array(items) => items.iter().any(json_has_crypt),
        _ => false,
    }
}

/// True if sshd has password authentication disabled and public-key
/// authentication enabled. `None` if `sshd -T` can't be run.
async fn check_password_auth_disabled() -> Option<bool> {
    let output = Command::new("sshd")
        .arg("-T")
        .output()
        .await
        .inspect_err(|e| error!("Failed to run sshd -T: {e}"))
        .ok()?;

    if !output.status.success() {
        warn!("sshd -T exited with status {:?}", output.status);
        return None;
    }

    // `sshd -T` prints effective directives as lowercase `key value` lines.
    let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
    Some(parse_password_auth_disabled(&stdout))
}

fn parse_password_auth_disabled(sshd_config: &str) -> bool {
    let mut password_off = false;
    let mut pubkey_on = false;
    for line in sshd_config.lines() {
        match line.trim() {
            "passwordauthentication no" => password_off = true,
            "pubkeyauthentication yes" => pubkey_on = true,
            _ => {}
        }
    }
    // Refuse to claim "hardened" unless key login is actually possible.
    password_off && pubkey_on
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_crypt_node_in_lsblk_tree() {
        let json = serde_json::json!({
            "blockdevices": [
                { "type": "disk", "children": [
                    { "type": "part", "children": [ { "type": "crypt" } ] }
                ]}
            ]
        });
        assert!(json_has_crypt(&json));
    }

    #[test]
    fn no_crypt_node_when_unencrypted() {
        let json = serde_json::json!({
            "blockdevices": [
                { "type": "disk", "children": [ { "type": "part" } ] }
            ]
        });
        assert!(!json_has_crypt(&json));
    }

    #[test]
    fn password_auth_disabled_requires_both_directives() {
        assert!(parse_password_auth_disabled(
            "passwordauthentication no\npubkeyauthentication yes\n"
        ));
        // Password off but key login also off => not safely hardened.
        assert!(!parse_password_auth_disabled(
            "passwordauthentication no\npubkeyauthentication no\n"
        ));
        assert!(!parse_password_auth_disabled(
            "passwordauthentication yes\npubkeyauthentication yes\n"
        ));
    }
}
