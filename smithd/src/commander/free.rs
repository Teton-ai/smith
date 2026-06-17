use crate::utils::schema::{SafeCommandResponse, SafeCommandRx};
use anyhow::{Context, Result};
use std::time::Duration;
use tokio::{process::Command, time::timeout};

pub(super) async fn execute_get_logs(
    id: i32,
    unit: Option<String>,
    since: Option<String>,
    until: Option<String>,
    grep: Option<String>,
) -> SafeCommandResponse {
    let args = build_journalctl_args(&unit, &since, &until, &grep);
    let future = Command::new("journalctl")
        .args(&args)
        .kill_on_drop(true)
        .output();

    match timeout(Duration::from_secs(60), future).await {
        Ok(Ok(output)) => {
            let (status_code, response) = process_output(output);
            SafeCommandResponse {
                id,
                command: response,
                status: status_code,
            }
        }
        Ok(Err(e)) => SafeCommandResponse {
            id,
            command: SafeCommandRx::FreeForm {
                stdout: String::new(),
                stderr: format!("Error: {}", e),
            },
            status: -1,
        },
        Err(_) => SafeCommandResponse {
            id,
            command: SafeCommandRx::FreeForm {
                stdout: String::new(),
                stderr: "Timeout running journalctl (60s)".to_string(),
            },
            status: -1,
        },
    }
}

fn build_journalctl_args(
    unit: &Option<String>,
    since: &Option<String>,
    until: &Option<String>,
    grep: &Option<String>,
) -> Vec<String> {
    let mut args = vec![
        "-r".to_string(),
        "--no-pager".to_string(),
        "-n".to_string(),
        "500".to_string(),
    ];
    if let Some(u) = unit {
        args.push("-u".to_string());
        args.push(u.clone());
    }
    if let Some(s) = since {
        args.push("--since".to_string());
        args.push(s.clone());
    }
    if let Some(u) = until {
        args.push("--until".to_string());
        args.push(u.clone());
    }
    if let Some(g) = grep {
        args.push("--grep".to_string());
        args.push(g.clone());
    }
    args
}

pub(super) async fn execute(id: i32, request: String) -> SafeCommandResponse {
    match execute_command(&request).await {
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
            command: SafeCommandRx::FreeForm {
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

    (status_code, SafeCommandRx::FreeForm { stdout, stderr })
}
