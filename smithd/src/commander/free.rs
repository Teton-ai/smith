use crate::utils::schema::{SafeCommandResponse, SafeCommandRx};
use anyhow::{Context, Result};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use std::process::Stdio;
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
    // Own process group: on timeout the whole tree can be signalled, not just `sh`,
    // which would otherwise leave children running and holding the output pipes.
    let child = Command::new("sh")
        .arg("-c")
        .arg(request)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0)
        .spawn()
        .context("Failed to run command")?;

    let pgid = child.id().map(|pid| Pid::from_raw(pid as i32));
    let wait = child.wait_with_output();
    tokio::pin!(wait);

    match timeout(Duration::from_secs(60), &mut wait).await {
        Ok(output) => output.context("Failed to run command"),
        Err(_) => {
            match pgid {
                Some(pgid) => {
                    if let Err(e) = killpg(pgid, Signal::SIGKILL) {
                        tracing::error!("Failed to kill command process group {pgid}: {e}");
                    }
                }
                None => tracing::error!("Timed-out command has no pid; cannot kill its group"),
            }
            // Drain the pipes and reap the tree before reporting the timeout.
            match timeout(Duration::from_secs(5), wait).await {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => tracing::error!("Failed to reap timed-out command: {e}"),
                Err(_) => tracing::error!("Timed out reaping the command process tree"),
            }
            Err(anyhow::anyhow!("Timeout running command (60s)"))
        }
    }
}

fn process_output(output: std::process::Output) -> (i32, SafeCommandRx) {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let status_code = output.status.code().unwrap_or(-1);

    (status_code, SafeCommandRx::FreeForm { stdout, stderr })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(
        unit: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
        grep: Option<&str>,
    ) -> Vec<String> {
        build_journalctl_args(
            &unit.map(str::to_string),
            &since.map(str::to_string),
            &until.map(str::to_string),
            &grep.map(str::to_string),
        )
    }

    #[test]
    fn defaults_only() {
        assert_eq!(
            args(None, None, None, None),
            ["-r", "--no-pager", "-n", "500"]
        );
    }

    #[test]
    fn unit_appended() {
        assert_eq!(
            args(Some("smithd"), None, None, None),
            ["-r", "--no-pager", "-n", "500", "-u", "smithd"]
        );
    }

    #[test]
    fn since_appended() {
        assert_eq!(
            args(None, Some("1h ago"), None, None),
            ["-r", "--no-pager", "-n", "500", "--since", "1h ago"]
        );
    }

    #[test]
    fn until_appended() {
        assert_eq!(
            args(None, None, Some("2026-06-17"), None),
            ["-r", "--no-pager", "-n", "500", "--until", "2026-06-17"]
        );
    }

    #[test]
    fn grep_appended() {
        assert_eq!(
            args(None, None, None, Some("error")),
            ["-r", "--no-pager", "-n", "500", "--grep", "error"]
        );
    }

    #[test]
    fn all_combined_ordering() {
        assert_eq!(
            args(
                Some("smithd"),
                Some("1h ago"),
                Some("2026-06-17"),
                Some("error")
            ),
            [
                "-r",
                "--no-pager",
                "-n",
                "500",
                "-u",
                "smithd",
                "--since",
                "1h ago",
                "--until",
                "2026-06-17",
                "--grep",
                "error"
            ]
        );
    }
}
