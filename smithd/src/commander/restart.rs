use crate::utils::schema::{SafeCommandRequest, SafeCommandResponse, SafeCommandRx};
use tokio::process::Command;

pub(super) async fn execute(request: &SafeCommandRequest) -> SafeCommandResponse {
    let cmd = Command::new("shutdown").arg("-r").arg("+1").output().await;

    match cmd {
        Ok(output) => {
            let status = output.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let details = [stdout.trim(), stderr.trim()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n");

            if status != 0 {
                tracing::warn!("shutdown -r +1 exited with {status}: {details}");
            }

            SafeCommandResponse {
                id: request.id,
                command: SafeCommandRx::Restart { message: details },
                status,
            }
        }
        Err(e) => {
            let status = -1;
            let details = format!("Error executing command: {}", e);
            SafeCommandResponse {
                id: request.id,
                command: SafeCommandRx::Restart { message: details },
                status,
            }
        }
    }
}
