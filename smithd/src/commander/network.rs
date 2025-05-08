use crate::utils::schema::{Network, SafeCommandResponse, SafeCommandRx};
use anyhow::{Context, Result};
use std::time::Duration;
use tokio::{process::Command, time::timeout};

pub(super) async fn execute(id: i32, network: Network) -> SafeCommandResponse {
    let optional_password = match network.password {
        Some(password) => format!("wifi-sec.key-mgmt wpa-psk wifi-sec.psk {password}"),
        None => String::default(),
    };

    let network_name = network.name;
    let network_ssid = network.ssid.unwrap_or(network_name.clone());

    // Initially attempt to connect to the network by network name. In case connecting
    // to the WiFi fails, add a new connection and make another attempt after.
    //
    // Before adding a connection, delete any existing connections with the same name.
    // This is done in order to cater use cases such as password changes.
    let command = format!(
        r#"nmcli c up {network_name} || \
          (nmcli connection delete {network_name}; \
           nmcli connection add \
           type wifi \
           con-name {network_name} \
           ssid {network_ssid} \
           autoconnect yes \
           connection.autoconnect-priority 500 \
           save yes \
           {optional_password} && \
           nmcli c up {network_name})"#
    );

    match execute_command(&command).await {
        Ok(output) => {
            let (status_code, response) = process_output(output);
            SafeCommandResponse {
                id,
                command: response,
                status: status_code,
            }
        }
        Err(e) => SafeCommandResponse {
            id,
            command: SafeCommandRx::WifiConnect {
                stdout: "".to_string(),
                stderr: format!("Error: {}", e),
            },
            status: -1,
        },
    }
}

async fn execute_command(request: &str) -> Result<std::process::Output> {
    let future = Command::new("sh")
        .arg("-c")
        .kill_on_drop(true)
        .arg(request)
        .output();

    match timeout(Duration::from_secs(60), future).await {
        Ok(output) => output.context("Failed to run command"),
        Err(_) => Err(anyhow::anyhow!("Timeout running command (60s)")),
    }
}

fn process_output(output: std::process::Output) -> (i32, SafeCommandRx) {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let status_code = output.status.code().unwrap_or(-1);

    (status_code, SafeCommandRx::WifiConnect { stdout, stderr })
}
