use crate::logstream::LogStreamHandle;
use crate::utils::schema::{SafeCommandResponse, SafeCommandRx};
use tracing::{error, info};

pub(super) async fn start_stream(
    id: i32,
    logstream_handle: &LogStreamHandle,
    service: Option<String>,
) -> SafeCommandResponse {
    info!("Starting log stream for service: {:?}", service);
    match logstream_handle.start_stream(service.clone()).await {
        Ok(port) => {
            info!("Log stream started successfully on port {}", port);
            SafeCommandResponse {
                id,
                command: SafeCommandRx::StartLogStream { port },
                status: 0,
            }
        }
        Err(e) => {
            error!("Failed to start log stream for service {:?}: {}", service, e);
            SafeCommandResponse {
                id,
                command: SafeCommandRx::StartLogStream { port: 0 },
                status: -1,
            }
        }
    }
}

pub(super) async fn stop_stream(id: i32, logstream_handle: &LogStreamHandle) -> SafeCommandResponse {
    logstream_handle.stop_stream().await;

    SafeCommandResponse {
        id,
        command: SafeCommandRx::StopLogStream,
        status: 0,
    }
}
