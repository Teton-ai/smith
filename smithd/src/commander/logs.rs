use crate::logstream::LogStreamHandle;
use crate::utils::schema::{SafeCommandResponse, SafeCommandRx};

pub(super) async fn start_stream(
    id: i32,
    handle: &LogStreamHandle,
    session_id: String,
    service_name: String,
    ws_url: String,
) -> SafeCommandResponse {
    match handle
        .start_stream(session_id.clone(), service_name, ws_url)
        .await
    {
        Ok(()) => SafeCommandResponse {
            id,
            command: SafeCommandRx::LogStreamStarted { session_id },
            status: 0,
        },
        Err(e) => SafeCommandResponse {
            id,
            command: SafeCommandRx::LogStreamError {
                session_id,
                error: e.to_string(),
            },
            status: -1,
        },
    }
}

pub(super) async fn stop_stream(
    id: i32,
    handle: &LogStreamHandle,
    session_id: String,
) -> SafeCommandResponse {
    handle.stop_stream(session_id.clone()).await;
    SafeCommandResponse {
        id,
        command: SafeCommandRx::LogStreamStopped { session_id },
        status: 0,
    }
}
