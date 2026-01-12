use crate::utils::schema::{ServiceStatus, ServiceToMonitor};
use chrono::{DateTime, Utc};
use std::process::Command;
use tracing::debug;

/// Check the status of a systemd service
pub fn check_service(service: &ServiceToMonitor) -> ServiceStatus {
    let active = is_service_active(&service.name);
    let uptime_sec = if active {
        get_service_uptime(&service.name)
    } else {
        None
    };

    let healthy = active
        && uptime_sec
            .map(|uptime| uptime >= service.watchdog_sec as u64)
            .unwrap_or(false);

    ServiceStatus {
        name: service.name.clone(),
        active,
        uptime_sec,
        healthy,
    }
}

/// Check all services and return their statuses
pub fn check_services(services: &[ServiceToMonitor]) -> Vec<ServiceStatus> {
    services.iter().map(check_service).collect()
}

/// Check if a systemd service is active using `systemctl is-active`
fn is_service_active(service_name: &str) -> bool {
    let output = Command::new("systemctl")
        .args(["is-active", service_name])
        .output();

    match output {
        Ok(output) => {
            let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
            debug!("Service {} is-active: {}", service_name, status);
            status == "active"
        }
        Err(e) => {
            debug!("Failed to check service {} status: {}", service_name, e);
            false
        }
    }
}

/// Get the uptime of a systemd service in seconds
/// Uses `systemctl show <service> --property=ActiveEnterTimestamp`
fn get_service_uptime(service_name: &str) -> Option<u64> {
    let output = Command::new("systemctl")
        .args(["show", service_name, "--property=ActiveEnterTimestamp"])
        .output();

    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_active_enter_timestamp(&stdout)
        }
        Err(e) => {
            debug!(
                "Failed to get ActiveEnterTimestamp for {}: {}",
                service_name, e
            );
            None
        }
    }
}

/// Parse the ActiveEnterTimestamp output from systemctl show
/// Example format: "ActiveEnterTimestamp=Mon 2024-01-15 10:30:00 UTC"
fn parse_active_enter_timestamp(output: &str) -> Option<u64> {
    let timestamp_str = output
        .lines()
        .find(|line| line.starts_with("ActiveEnterTimestamp="))?
        .strip_prefix("ActiveEnterTimestamp=")?
        .trim();

    if timestamp_str.is_empty() {
        return None;
    }

    // Parse the timestamp (format: "Mon 2024-01-15 10:30:00 UTC")
    let parsed = DateTime::parse_from_str(timestamp_str, "%a %Y-%m-%d %H:%M:%S %Z")
        .ok()
        .map(|dt| dt.with_timezone(&Utc));

    if let Some(start_time) = parsed {
        let now = Utc::now();
        let duration = now.signed_duration_since(start_time);
        if duration.num_seconds() >= 0 {
            return Some(duration.num_seconds() as u64);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_active_enter_timestamp() {
        // Empty timestamp
        assert_eq!(parse_active_enter_timestamp("ActiveEnterTimestamp="), None);

        // No prefix
        assert_eq!(parse_active_enter_timestamp("SomeOtherProperty=value"), None);
    }
}
