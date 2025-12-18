use crate::magic::MagicHandle;
use crate::shutdown::ShutdownHandler;
use crate::utils::schema::{Network, SafeCommandResponse, SafeCommandRx};
use anyhow::{Context, Result};
use reqwest::Client;
use std::time::{Duration, Instant};
use tokio::{process::Command, time::timeout};

pub(super) async fn execute(id: i32, network: Network) -> SafeCommandResponse {
    let network_name = network.name;
    let network_ssid = network.ssid.unwrap_or(network_name.clone());

    // Initially attempt to connect to the network by network name. In case connecting
    // to the WiFi fails, add a new connection and make another attempt after.
    //
    // Before adding a connection, delete any existing connections with the same name.
    // This is done in order to cater use cases such as password changes.

    // Try to connect to existing connection
    let mut cmd = Command::new("nmcli");
    cmd.arg("c").arg("up").arg(&network_name);

    if let Ok(output) = execute_nmcli_command(cmd).await
        && output.status.success()
    {
        let (status_code, response) = process_output(output);
        return SafeCommandResponse {
            id,
            command: response,
            status: status_code,
        };
    }
    // Connection failed, delete old connection and create new one
    // Delete existing connection (ignore errors if it doesn't exist)
    let mut delete_cmd = Command::new("nmcli");
    delete_cmd
        .arg("connection")
        .arg("delete")
        .arg(&network_name);
    let _ = execute_nmcli_command(delete_cmd).await;

    // Add new connection
    let mut add_cmd = Command::new("nmcli");
    add_cmd.args([
        "connection",
        "add",
        "type",
        "wifi",
        "con-name",
        &network_name,
        "ssid",
        &network_ssid,
        "autoconnect",
        "yes",
        "connection.autoconnect-priority",
        "500",
        "save",
        "yes",
    ]);

    if let Some(password) = network.password {
        add_cmd.args(["wifi-sec.key-mgmt", "wpa-psk", "wifi-sec.psk", &password]);
    }

    match execute_nmcli_command(add_cmd).await {
        Ok(output) if output.status.success() => {
            // Connection added successfully, now connect
            let mut connect_cmd = Command::new("nmcli");
            connect_cmd.args(["c", "up"]).arg(&network_name);

            match execute_nmcli_command(connect_cmd).await {
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
                        stderr: format!("Error connecting after adding connection: {}", e),
                    },
                    status: -1,
                },
            }
        }
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
                stderr: format!("Error adding connection: {}", e),
            },
            status: -1,
        },
    }
}

async fn execute_nmcli_command(mut cmd: Command) -> Result<std::process::Output> {
    let future = cmd.kill_on_drop(true).output();

    match timeout(Duration::from_secs(60), future).await {
        Ok(output) => output.context("Failed to run nmcli command"),
        Err(_) => Err(anyhow::anyhow!("Timeout running nmcli command (60s)")),
    }
}

fn process_output(output: std::process::Output) -> (i32, SafeCommandRx) {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let status_code = output.status.code().unwrap_or(-1);

    (status_code, SafeCommandRx::WifiConnect { stdout, stderr })
}

pub(super) async fn test_network(id: i32) -> SafeCommandResponse {
    let result = perform_network_test().await;
    SafeCommandResponse {
        id,
        command: SafeCommandRx::TestNetwork {
            bytes_downloaded: result.bytes_downloaded,
            duration_ms: result.duration_ms,
            bytes_uploaded: result.bytes_uploaded,
            upload_duration_ms: result.upload_duration_ms,
            timed_out: result.timed_out,
        },
        status: if result.timed_out || result.error {
            -1
        } else {
            0
        },
    }
}

const NETWORK_TEST_TIMEOUT: Duration = Duration::from_secs(30);

struct NetworkTestResult {
    bytes_downloaded: usize,
    duration_ms: u64,
    bytes_uploaded: Option<usize>,
    upload_duration_ms: Option<u64>,
    timed_out: bool,
    error: bool,
}

async fn perform_network_test() -> NetworkTestResult {
    match perform_network_test_inner().await {
        Ok(result) => result,
        Err(e) => {
            tracing::error!("Network test failed: {}", e);
            NetworkTestResult {
                bytes_downloaded: 0,
                duration_ms: 0,
                bytes_uploaded: None,
                upload_duration_ms: None,
                timed_out: false,
                error: true,
            }
        }
    }
}

async fn perform_network_test_inner() -> Result<NetworkTestResult> {
    let shutdown = ShutdownHandler::new();
    let configuration = MagicHandle::new(shutdown.signals());
    configuration.load(None).await;

    let server_api_url = configuration.get_server().await;
    let download_url = format!("{}/network/test-file", server_api_url);
    let upload_url = format!("{}/network/test-upload", server_api_url);

    // No global timeout on client - we handle timeouts per-phase
    let client = Client::builder()
        .build()
        .context("Failed to create HTTP client")?;

    // Download test with timeout
    let download_start = Instant::now();
    let mut downloaded: usize = 0;
    let mut download_timed_out = false;

    let response = match timeout(NETWORK_TEST_TIMEOUT, client.get(&download_url).send()).await {
        Ok(Ok(resp)) => resp,
        Ok(Err(e)) => return Err(e.into()),
        Err(_) => {
            return Ok(NetworkTestResult {
                bytes_downloaded: 0,
                duration_ms: NETWORK_TEST_TIMEOUT.as_millis() as u64,
                bytes_uploaded: None,
                upload_duration_ms: None,
                timed_out: true,
                error: false,
            });
        }
    };

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Server returned status: {}",
            response.status()
        ));
    }

    let mut stream = response.bytes_stream();
    use futures_util::StreamExt;

    loop {
        let remaining = NETWORK_TEST_TIMEOUT.saturating_sub(download_start.elapsed());
        if remaining.is_zero() {
            download_timed_out = true;
            break;
        }

        match timeout(remaining, stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                downloaded += chunk.len();
            }
            Ok(Some(Err(e))) => return Err(e.into()),
            Ok(None) => break, // Stream finished
            Err(_) => {
                download_timed_out = true;
                break;
            }
        }
    }

    let download_duration_ms = download_start.elapsed().as_millis() as u64;

    // If download timed out, return partial results (skip upload)
    if download_timed_out {
        return Ok(NetworkTestResult {
            bytes_downloaded: downloaded,
            duration_ms: download_duration_ms,
            bytes_uploaded: None,
            upload_duration_ms: None,
            timed_out: true,
            error: false,
        });
    }

    // Upload test with timeout
    let upload_data = vec![0u8; downloaded];
    let upload_start = Instant::now();

    let upload_result = timeout(
        NETWORK_TEST_TIMEOUT,
        client.post(&upload_url).body(upload_data).send(),
    )
    .await;

    match upload_result {
        Ok(Ok(resp)) if resp.status().is_success() => {
            let upload_duration_ms = upload_start.elapsed().as_millis() as u64;
            Ok(NetworkTestResult {
                bytes_downloaded: downloaded,
                duration_ms: download_duration_ms,
                bytes_uploaded: Some(downloaded),
                upload_duration_ms: Some(upload_duration_ms),
                timed_out: false,
                error: false,
            })
        }
        Ok(Ok(resp)) => Err(anyhow::anyhow!(
            "Upload server returned status: {}",
            resp.status()
        )),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => {
            // Upload timed out - return download results with upload timeout
            Ok(NetworkTestResult {
                bytes_downloaded: downloaded,
                duration_ms: download_duration_ms,
                bytes_uploaded: Some(0),
                upload_duration_ms: Some(NETWORK_TEST_TIMEOUT.as_millis() as u64),
                timed_out: true,
                error: false,
            })
        }
    }
}
