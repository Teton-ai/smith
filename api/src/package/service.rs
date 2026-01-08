use std::io::{BufRead, BufReader, Read};
use tracing::debug;

#[derive(Debug)]
pub struct ServiceInfo {
    pub name: String,
    pub watchdog_sec: Option<i32>,
}

/// Parses a systemd .service file content to extract WatchdogSec value.
/// Returns the WatchdogSec value in seconds if found in the [Service] section.
pub fn parse_service_file<R: Read>(reader: R) -> Option<i32> {
    let buf_reader = BufReader::new(reader);
    let mut in_service_section = false;

    for line in buf_reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let trimmed = line.trim();

        // Track section headers
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_service_section = trimmed == "[Service]";
            continue;
        }

        // Only look for WatchdogSec in [Service] section
        if in_service_section {
            if let Some(value) = trimmed.strip_prefix("WatchdogSec=") {
                return parse_watchdog_value(value.trim());
            }
        }
    }

    None
}

/// Parses WatchdogSec value which can be a number or time string.
/// Supports formats: "30", "30s", "1min", "500ms", "1h"
fn parse_watchdog_value(value: &str) -> Option<i32> {
    // Try direct integer parse first
    if let Ok(secs) = value.parse::<i32>() {
        return Some(secs);
    }

    let value_lower = value.to_lowercase();

    // Handle common time suffixes
    if let Some(num_str) = value_lower.strip_suffix("ms") {
        // Milliseconds - convert to seconds (minimum 1)
        return num_str
            .trim()
            .parse::<i32>()
            .ok()
            .map(|ms| (ms / 1000).max(1));
    }

    if let Some(num_str) = value_lower.strip_suffix("sec") {
        return num_str.trim().parse::<i32>().ok();
    }

    if let Some(num_str) = value_lower.strip_suffix('s') {
        return num_str.trim().parse::<i32>().ok();
    }

    if let Some(num_str) = value_lower.strip_suffix("min") {
        return num_str.trim().parse::<i32>().ok().map(|m| m * 60);
    }

    if let Some(num_str) = value_lower.strip_suffix('m') {
        return num_str.trim().parse::<i32>().ok().map(|m| m * 60);
    }

    if let Some(num_str) = value_lower.strip_suffix('h') {
        return num_str.trim().parse::<i32>().ok().map(|h| h * 3600);
    }

    debug!("Could not parse WatchdogSec value: {}", value);
    None
}

/// Extracts service name from file path.
/// e.g., "/lib/systemd/system/myapp.service" -> "myapp"
pub fn extract_service_name(path: &str) -> Option<String> {
    let filename = path.rsplit('/').next()?;
    filename.strip_suffix(".service").map(|s| s.to_string())
}

/// Checks if a path is a systemd service file location.
pub fn is_service_file_path(path: &str) -> bool {
    let service_dirs = [
        "lib/systemd/system/",
        "usr/lib/systemd/system/",
        "etc/systemd/system/",
    ];

    // Remove leading slash or "./" for comparison
    let normalized = path.trim_start_matches('/').trim_start_matches("./");

    service_dirs.iter().any(|dir| normalized.starts_with(dir)) && normalized.ends_with(".service")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_parse_service_file_with_watchdog() {
        let content = r#"
[Unit]
Description=Test Service

[Service]
Type=notify
ExecStart=/usr/bin/test
WatchdogSec=30

[Install]
WantedBy=multi-user.target
"#;
        let cursor = Cursor::new(content);
        assert_eq!(parse_service_file(cursor), Some(30));
    }

    #[test]
    fn test_parse_service_file_with_watchdog_suffix() {
        let content = r#"
[Service]
WatchdogSec=15s
"#;
        let cursor = Cursor::new(content);
        assert_eq!(parse_service_file(cursor), Some(15));
    }

    #[test]
    fn test_parse_service_file_with_minutes() {
        let content = r#"
[Service]
WatchdogSec=2min
"#;
        let cursor = Cursor::new(content);
        assert_eq!(parse_service_file(cursor), Some(120));
    }

    #[test]
    fn test_parse_service_file_without_watchdog() {
        let content = r#"
[Unit]
Description=Test Service

[Service]
Type=simple
ExecStart=/usr/bin/test

[Install]
WantedBy=multi-user.target
"#;
        let cursor = Cursor::new(content);
        assert_eq!(parse_service_file(cursor), None);
    }

    #[test]
    fn test_parse_service_file_watchdog_outside_service_section() {
        let content = r#"
[Unit]
WatchdogSec=30

[Service]
Type=simple
"#;
        let cursor = Cursor::new(content);
        assert_eq!(parse_service_file(cursor), None);
    }

    #[test]
    fn test_is_service_file_path() {
        assert!(is_service_file_path("/lib/systemd/system/myapp.service"));
        assert!(is_service_file_path(
            "./usr/lib/systemd/system/other.service"
        ));
        assert!(is_service_file_path("etc/systemd/system/test.service"));
        assert!(!is_service_file_path("/usr/bin/myapp"));
        assert!(!is_service_file_path("/lib/systemd/system/myapp.socket"));
        assert!(!is_service_file_path("/lib/systemd/system/myapp.timer"));
    }

    #[test]
    fn test_extract_service_name() {
        assert_eq!(
            extract_service_name("/lib/systemd/system/myapp.service"),
            Some("myapp".to_string())
        );
        assert_eq!(
            extract_service_name("./etc/systemd/system/my-daemon.service"),
            Some("my-daemon".to_string())
        );
        assert_eq!(extract_service_name("/usr/bin/myapp"), None);
    }

    #[test]
    fn test_parse_watchdog_milliseconds() {
        assert_eq!(parse_watchdog_value("500ms"), Some(1)); // Minimum 1 second
        assert_eq!(parse_watchdog_value("5000ms"), Some(5));
    }

    #[test]
    fn test_parse_watchdog_hours() {
        assert_eq!(parse_watchdog_value("1h"), Some(3600));
    }
}
