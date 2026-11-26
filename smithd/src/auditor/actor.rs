use crate::commander::CommanderHandle;
use crate::shutdown::ShutdownSignals;
use crate::utils::schema::{
    RootfsSlot, SafeCommandResponse, SafeCommandRx, UdaEncryption, UefiSecureBoot,
};
use std::path::Path;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};
use tracing::{error, info, warn};

/// Run a full audit every 12 hours.
const AUDIT_INTERVAL_SECS: u64 = 12 * 60 * 60;

/// Hard cap on each compliance probe subprocess so a hung command can't block
/// audit processing or delay actor shutdown indefinitely.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

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
        self.commander
            .insert_result(vec![SafeCommandResponse {
                id: AUDIT_RESULT_ID,
                command: run_audit_checks().await,
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

/// Run the host compliance checks into an `AuditReport`. Each value is `None`
/// when its probe couldn't be run or parsed.
pub async fn run_audit_checks() -> SafeCommandRx {
    let (disk_encrypted, root_encrypted, uda_encryption) = check_block_devices().await;
    let password_access_disabled = check_password_auth_disabled().await;
    let console_logins_closed = crate::hardening::console_logins_closed().await;
    let reboot_watchdog_set = crate::hardening::reboot_watchdog_set().await;
    let (rootfs_slot, rootfs_other_slot_bootable) = check_rootfs_slots().await;
    let uefi_secure_boot = check_uefi_secure_boot(Path::new(EFI_DIR)).await;

    info!(
        ?disk_encrypted,
        ?password_access_disabled,
        ?console_logins_closed,
        ?reboot_watchdog_set,
        ?rootfs_slot,
        ?rootfs_other_slot_bootable,
        ?uefi_secure_boot,
        ?root_encrypted,
        ?uda_encryption,
        "Audit complete"
    );

    SafeCommandRx::AuditReport {
        disk_encrypted,
        password_access_disabled,
        console_logins_closed,
        reboot_watchdog_set,
        rootfs_slot,
        rootfs_other_slot_bootable,
        uefi_secure_boot,
        root_encrypted,
        uda_encryption,
    }
}

const EFI_DIR: &str = "/sys/firmware/efi";
/// uefi global variable guid, `SecureBoot` and `SetupMode` live under it
const EFI_GLOBAL_GUID: &str = "8be4df61-93ca-11d2-aa0d-00e098032b8c";

/// `None` when the device didn't boot through uefi, there's nothing to say then.
/// `efi_dir` is a parameter so tests can point it at a fake tree
async fn check_uefi_secure_boot(efi_dir: &Path) -> Option<UefiSecureBoot> {
    if !tokio::fs::try_exists(efi_dir)
        .await
        .inspect_err(|e| warn!("Failed to check {}: {e}", efi_dir.display()))
        .ok()?
    {
        return None;
    }

    let vars = efi_dir.join("efivars");
    let secure_boot = read_efi_bool(&vars, "SecureBoot").await;
    let setup_mode = read_efi_bool(&vars, "SetupMode").await;
    Some(secure_boot_state(secure_boot?, setup_mode?))
}

/// efivarfs files are 4 attribute bytes then the value. a missing variable reads
/// as false: firmware without secure boot support doesn't define them.
/// `None` only when the variable exists but can't be read
async fn read_efi_bool(vars: &Path, name: &str) -> Option<bool> {
    let path = vars.join(format!("{name}-{EFI_GLOBAL_GUID}"));
    match tokio::fs::read(&path).await {
        Ok(bytes) => match bytes.get(4) {
            Some(value) => Some(*value == 1),
            None => {
                warn!("{} is too short to hold a value", path.display());
                None
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(false),
        Err(e) => {
            warn!("Failed to read {}: {e}", path.display());
            None
        }
    }
}

/// in setup mode secure boot can't be enforcing, whatever `SecureBoot` says
fn secure_boot_state(secure_boot: bool, setup_mode: bool) -> UefiSecureBoot {
    if setup_mode {
        UefiSecureBoot::Setup
    } else if secure_boot {
        UefiSecureBoot::On
    } else {
        UefiSecureBoot::Off
    }
}

/// Booted rootfs slot and whether the other one could still boot, from
/// nvidia's `nvbootctrl`. Both `None` off jetson or if the output can't be read.
async fn check_rootfs_slots() -> (Option<RootfsSlot>, Option<bool>) {
    let output = match time::timeout(
        PROBE_TIMEOUT,
        Command::new("nvbootctrl")
            .args(["-t", "rootfs", "dump-slots-info"])
            .output(),
    )
    .await
    {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            warn!("Failed to run nvbootctrl: {e}");
            return (None, None);
        }
        Err(_) => {
            error!("nvbootctrl timed out after {PROBE_TIMEOUT:?}");
            return (None, None);
        }
    };

    if !output.status.success() {
        warn!("nvbootctrl exited with status {:?}", output.status);
        return (None, None);
    }

    parse_rootfs_slots(&String::from_utf8_lossy(&output.stdout))
}

/// Parses `nvbootctrl -t rootfs dump-slots-info`:
/// ```text
/// Current rootfs slot: A
/// Active rootfs slot: A
/// num_slots: 2
/// slot: 0,             retry_count: 2,             status: normal
/// slot: 1,             retry_count: 2,             status: normal
/// ```
fn parse_rootfs_slots(output: &str) -> (Option<RootfsSlot>, Option<bool>) {
    let mut current = None;
    let mut num_slots = None;
    let mut statuses = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if let Some(slot) = line.strip_prefix("Current rootfs slot:") {
            current = match slot.trim() {
                "A" => Some(0),
                "B" => Some(1),
                _ => None,
            };
        } else if let Some(n) = line.strip_prefix("num_slots:") {
            num_slots = n.trim().parse::<u32>().ok();
        } else if line.starts_with("slot:") {
            let mut index = None;
            let mut status = None;
            for field in line.split(',') {
                match field.trim().split_once(':') {
                    Some(("slot", v)) => index = v.trim().parse::<usize>().ok(),
                    Some(("status", v)) => status = Some(v.trim().to_owned()),
                    _ => {}
                }
            }
            if let (Some(index), Some(status)) = (index, status) {
                statuses.push((index, status));
            }
        }
    }

    // (untested) assumes a unit without rootfs a/b reports a single slot
    if num_slots == Some(1) {
        return (Some(RootfsSlot::Single), None);
    }
    let Some(current) = current else {
        return (None, None);
    };
    let slot = if current == 0 {
        RootfsSlot::A
    } else {
        RootfsSlot::B
    };

    let other = 1 - current;
    let other_bootable =
        statuses
            .iter()
            .find(|(index, _)| *index == other)
            .and_then(|(_, status)| match status.as_str() {
                "normal" => Some(true),
                "unbootable" => Some(false),
                unknown => {
                    warn!("Unknown rootfs slot status {unknown:?}");
                    None
                }
            });

    (Some(slot), other_bootable)
}

/// `(disk_encrypted, root_encrypted, uda_encryption)` from one `lsblk` call:
/// any crypt mapping at all, whether `/` sits on one, and whether the `UDA`
/// partition does. All `None` if lsblk can't be run or parsed.
async fn check_block_devices() -> (Option<bool>, Option<bool>, Option<UdaEncryption>) {
    let Some(json) = lsblk_tree().await else {
        return (None, None, None);
    };
    (
        Some(json_has_crypt(&json)),
        root_on_crypt(&json),
        Some(uda_encryption(&json)),
    )
}

async fn lsblk_tree() -> Option<serde_json::Value> {
    let output = time::timeout(
        PROBE_TIMEOUT,
        Command::new("lsblk")
            .args(["-J", "-o", "NAME,TYPE,PARTLABEL,MOUNTPOINT"])
            .output(),
    )
    .await
    .inspect_err(|_| error!("lsblk timed out after {PROBE_TIMEOUT:?}"))
    .ok()?
    .inspect_err(|e| error!("Failed to run lsblk: {e}"))
    .ok()?;

    if !output.status.success() {
        warn!("lsblk exited with status {:?}", output.status);
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout)
        .inspect_err(|e| error!("Failed to parse lsblk JSON: {e}"))
        .ok()
}

fn lsblk_children(node: &serde_json::Value) -> impl Iterator<Item = &serde_json::Value> {
    node.get("children")
        .and_then(|c| c.as_array())
        .into_iter()
        .flatten()
}

fn lsblk_field<'a>(node: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    node.get(key).and_then(|v| v.as_str())
}

/// lsblk nests a crypt mapping under its partition, so `/` is encrypted when the
/// node mounted there or anything above it is a crypt device. `None` if nothing
/// is mounted at `/`
fn root_on_crypt(json: &serde_json::Value) -> Option<bool> {
    fn walk(node: &serde_json::Value, crypt_above: bool) -> Option<bool> {
        let crypt = crypt_above || lsblk_field(node, "type") == Some("crypt");
        if lsblk_field(node, "mountpoint") == Some("/") {
            return Some(crypt);
        }
        lsblk_children(node).find_map(|child| walk(child, crypt))
    }
    json.get("blockdevices")?
        .as_array()?
        .iter()
        .find_map(|device| walk(device, false))
}

/// encrypted when the partition labelled `UDA` has a crypt mapping under it
fn uda_encryption(json: &serde_json::Value) -> UdaEncryption {
    fn find_uda(node: &serde_json::Value) -> Option<&serde_json::Value> {
        if lsblk_field(node, "partlabel") == Some("UDA") {
            return Some(node);
        }
        lsblk_children(node).find_map(find_uda)
    }
    let uda = json
        .get("blockdevices")
        .and_then(|d| d.as_array())
        .into_iter()
        .flatten()
        .find_map(find_uda);
    match uda {
        Some(partition) if lsblk_children(partition).any(json_has_crypt) => {
            UdaEncryption::Encrypted
        }
        Some(_) => UdaEncryption::Unencrypted,
        None => UdaEncryption::Absent,
    }
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
    let output = time::timeout(PROBE_TIMEOUT, Command::new("sshd").arg("-T").output())
        .await
        .inspect_err(|_| error!("sshd -T timed out after {PROBE_TIMEOUT:?}"))
        .ok()?
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

    // verbatim from the dev unit on 2026-11-13
    const TWO_SLOTS_ON_A: &str = "Current rootfs slot: A
Active rootfs slot: A
num_slots: 2
slot: 0,             retry_count: 2,             status: normal
slot: 1,             retry_count: 2,             status: normal
";

    #[test]
    fn rootfs_on_a_with_b_bootable() {
        assert_eq!(
            parse_rootfs_slots(TWO_SLOTS_ON_A),
            (Some(RootfsSlot::A), Some(true))
        );
    }

    #[test]
    fn rootfs_on_b_with_a_unbootable() {
        let output = TWO_SLOTS_ON_A
            .replace("Current rootfs slot: A", "Current rootfs slot: B")
            .replace(
                "slot: 0,             retry_count: 2,             status: normal",
                "slot: 0,             retry_count: 0,             status: unbootable",
            );
        assert_eq!(
            parse_rootfs_slots(&output),
            (Some(RootfsSlot::B), Some(false))
        );
    }

    #[test]
    fn single_rootfs_slot() {
        let output =
            "Current rootfs slot: A\nnum_slots: 1\nslot: 0, retry_count: 3, status: normal\n";
        assert_eq!(parse_rootfs_slots(output), (Some(RootfsSlot::Single), None));
    }

    #[test]
    fn unknown_other_slot_status_is_none() {
        let output = TWO_SLOTS_ON_A.replace(
            "slot: 1,             retry_count: 2,             status: normal",
            "slot: 1,             retry_count: 2,             status: something-new",
        );
        assert_eq!(parse_rootfs_slots(&output), (Some(RootfsSlot::A), None));
    }

    /// the dev unit's nvme layout on 2026-11-13, trimmed, with the mountpoints
    /// lsblk adds for MOUNTPOINT
    fn dev_unit_lsblk() -> serde_json::Value {
        serde_json::json!({"blockdevices": [
            {"name": "loop0", "type": "loop", "partlabel": null, "mountpoint": null},
            {"name": "nvme0n1", "type": "disk", "partlabel": null, "mountpoint": null, "children": [
                {"name": "nvme0n1p1", "type": "part", "partlabel": "APP", "mountpoint": null},
                {"name": "nvme0n1p2", "type": "part", "partlabel": "APP_b", "mountpoint": null},
                {"name": "nvme0n1p3", "type": "part", "partlabel": "APP_ENC", "mountpoint": null, "children": [
                    {"name": "crypt_root", "type": "crypt", "partlabel": null, "mountpoint": "/"}
                ]},
                {"name": "nvme0n1p4", "type": "part", "partlabel": "APP_ENC_b", "mountpoint": null, "children": [
                    {"name": "crypt_root_other", "type": "crypt", "partlabel": null, "mountpoint": null}
                ]},
                {"name": "nvme0n1p17", "type": "part", "partlabel": "UDA", "mountpoint": null, "children": [
                    {"name": "crypt_UDA", "type": "crypt", "partlabel": null, "mountpoint": "/mnt/crypt_UDA"}
                ]}
            ]}
        ]})
    }

    #[test]
    fn dev_unit_root_and_uda_encrypted() {
        let json = dev_unit_lsblk();
        assert!(json_has_crypt(&json));
        assert_eq!(root_on_crypt(&json), Some(true));
        assert_eq!(uda_encryption(&json), UdaEncryption::Encrypted);
    }

    #[test]
    fn plain_root_and_plain_uda() {
        let json = serde_json::json!({"blockdevices": [
            {"name": "nvme0n1", "type": "disk", "partlabel": null, "mountpoint": null, "children": [
                {"name": "nvme0n1p1", "type": "part", "partlabel": "APP", "mountpoint": "/"},
                {"name": "nvme0n1p17", "type": "part", "partlabel": "UDA", "mountpoint": "/data"}
            ]}
        ]});
        assert!(!json_has_crypt(&json));
        assert_eq!(root_on_crypt(&json), Some(false));
        assert_eq!(uda_encryption(&json), UdaEncryption::Unencrypted);
    }

    #[test]
    fn encrypted_uda_does_not_make_a_plain_root_pass() {
        // the gap the old any-crypt check had
        let json = serde_json::json!({"blockdevices": [
            {"name": "nvme0n1", "type": "disk", "partlabel": null, "mountpoint": null, "children": [
                {"name": "nvme0n1p1", "type": "part", "partlabel": "APP", "mountpoint": "/"},
                {"name": "nvme0n1p17", "type": "part", "partlabel": "UDA", "mountpoint": null, "children": [
                    {"name": "crypt_UDA", "type": "crypt", "partlabel": null, "mountpoint": "/mnt/crypt_UDA"}
                ]}
            ]}
        ]});
        assert!(json_has_crypt(&json));
        assert_eq!(root_on_crypt(&json), Some(false));
        assert_eq!(uda_encryption(&json), UdaEncryption::Encrypted);
    }

    #[test]
    fn no_uda_partition_is_absent() {
        let json = serde_json::json!({"blockdevices": [
            {"name": "mmcblk0", "type": "disk", "partlabel": null, "mountpoint": null, "children": [
                {"name": "mmcblk0p1", "type": "part", "partlabel": "APP", "mountpoint": "/"}
            ]}
        ]});
        assert_eq!(uda_encryption(&json), UdaEncryption::Absent);
    }

    #[test]
    fn nothing_mounted_at_root_is_none() {
        let json = serde_json::json!({"blockdevices": [
            {"name": "sda", "type": "disk", "partlabel": null, "mountpoint": "/mnt/usb"}
        ]});
        assert_eq!(root_on_crypt(&json), None);
    }

    /// fake efi tree with the given variables, values as efivarfs stores them
    fn efi_tree(vars: &[(&str, u8)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let efivars = dir.path().join("efivars");
        std::fs::create_dir(&efivars).unwrap();
        for (name, value) in vars {
            std::fs::write(
                efivars.join(format!("{name}-{EFI_GLOBAL_GUID}")),
                [6, 0, 0, 0, *value],
            )
            .unwrap();
        }
        dir
    }

    #[tokio::test]
    async fn secure_boot_on_like_the_dev_unit() {
        // the dev unit read 6 0 0 0 1 and 6 0 0 0 0 on 2026-11-13
        let dir = efi_tree(&[("SecureBoot", 1), ("SetupMode", 0)]);
        assert_eq!(
            check_uefi_secure_boot(dir.path()).await,
            Some(UefiSecureBoot::On)
        );
    }

    #[tokio::test]
    async fn setup_mode_is_not_on() {
        let dir = efi_tree(&[("SecureBoot", 0), ("SetupMode", 1)]);
        assert_eq!(
            check_uefi_secure_boot(dir.path()).await,
            Some(UefiSecureBoot::Setup)
        );
    }

    #[tokio::test]
    async fn secure_boot_off() {
        let dir = efi_tree(&[("SecureBoot", 0), ("SetupMode", 0)]);
        assert_eq!(
            check_uefi_secure_boot(dir.path()).await,
            Some(UefiSecureBoot::Off)
        );
    }

    #[tokio::test]
    async fn uefi_without_secure_boot_variables_is_off() {
        let dir = efi_tree(&[]);
        assert_eq!(
            check_uefi_secure_boot(dir.path()).await,
            Some(UefiSecureBoot::Off)
        );
    }

    #[tokio::test]
    async fn no_uefi_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("efi");
        assert_eq!(check_uefi_secure_boot(&missing).await, None);
    }

    #[test]
    fn unreadable_output_is_none() {
        assert_eq!(parse_rootfs_slots("garbage"), (None, None));
        assert_eq!(parse_rootfs_slots(""), (None, None));
    }

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
