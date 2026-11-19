//! Reboots the device, escalating until something works. On v2 hardware the
//! power board cuts VIN, so the Ethernet switch and PoE cameras restart too.

use std::time::Duration;
use tracing::{error, info, warn};

/// `esp-host system reset power-board power-reset`: the ESP32 pulls the power
/// board's reset pin, which cycles VIN of the whole unit.
const ESP_HOST: &str = "esp-host";
const POWER_RESET: [&str; 4] = ["system", "reset", "power-board", "power-reset"];

/// What the reboot needs from the device, so the order can be tested.
pub trait Device {
    fn has_program(&self, program: &str) -> bool;
    /// Runs `program` and returns whether it exited 0 before `timeout`.
    fn run(&mut self, program: &str, args: &[&str], timeout: Duration) -> bool;
    /// Waits, and keeps the systemd watchdog fed.
    fn wait(&mut self, duration: Duration);
    fn sync(&mut self);
    /// The `reboot(2)` syscall. Returns only when it fails.
    fn force_reboot(&mut self);
}

/// Each step waits for the one before it to take the device down. The device
/// is still up after a step only when that step failed.
pub fn reboot(device: &mut impl Device) {
    device.sync();

    if device.has_program(ESP_HOST) {
        info!("Cycling the power board");
        if !device.run(ESP_HOST, &POWER_RESET, Duration::from_secs(30)) {
            warn!("The power board reset failed");
        }
        device.wait(Duration::from_secs(60));
        warn!("Still up after the power board reset; rebooting the computer");
    }

    info!("Rebooting with systemd");
    if !device.run("systemctl", &["reboot"], Duration::from_secs(60)) {
        warn!("systemctl reboot failed");
    }
    device.wait(Duration::from_secs(180));

    // Skips stopping services, for a systemd that cannot finish a shutdown.
    warn!("Still up; forcing the reboot");
    if !device.run(
        "systemctl",
        &["reboot", "--force", "--force"],
        Duration::from_secs(30),
    ) {
        warn!("systemctl reboot --force --force failed");
    }
    device.wait(Duration::from_secs(60));

    // Works when PID 1 is stuck. In a container it ends only the container.
    error!("Still up; calling reboot(2)");
    device.sync();
    device.force_reboot();
    error!("reboot(2) failed; trying again on the next poll");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records each step. No step takes the device down, so `reboot` runs all
    /// of them, as on a device where each one fails.
    #[derive(Default)]
    struct Fake {
        esp_host: bool,
        steps: Vec<String>,
    }

    impl Device for Fake {
        fn has_program(&self, program: &str) -> bool {
            program == ESP_HOST && self.esp_host
        }
        fn run(&mut self, program: &str, args: &[&str], _timeout: Duration) -> bool {
            self.steps.push(format!("{program} {}", args.join(" ")));
            false
        }
        fn wait(&mut self, duration: Duration) {
            self.steps.push(format!("wait {}s", duration.as_secs()));
        }
        fn sync(&mut self) {
            self.steps.push("sync".into());
        }
        fn force_reboot(&mut self) {
            self.steps.push("reboot(2)".into());
        }
    }

    #[test]
    fn v2_hardware_cycles_the_power_board_first() {
        let mut device = Fake {
            esp_host: true,
            ..Fake::default()
        };
        reboot(&mut device);
        assert_eq!(
            device.steps,
            [
                "sync",
                "esp-host system reset power-board power-reset",
                "wait 60s",
                "systemctl reboot",
                "wait 180s",
                "systemctl reboot --force --force",
                "wait 60s",
                "sync",
                "reboot(2)",
            ]
        );
    }

    #[test]
    fn without_esp_host_the_computer_reboots() {
        let mut device = Fake::default();
        reboot(&mut device);
        assert_eq!(device.steps[..2], ["sync", "systemctl reboot"]);
        assert_eq!(device.steps.last().map(String::as_str), Some("reboot(2)"));
    }
}
