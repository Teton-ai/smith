use crate::filebrowser::FileBrowserHandle;
use crate::utils::schema::{SafeCommandResponse, SafeCommandRx};

pub(super) async fn open_session(
    id: i32,
    handle: &FileBrowserHandle,
    session_id: String,
) -> SafeCommandResponse {
    match handle.open_session(session_id.clone()).await {
        Ok(()) => SafeCommandResponse {
            id,
            command: SafeCommandRx::FileSessionStarted { session_id },
            status: 0,
        },
        Err(e) => SafeCommandResponse {
            id,
            command: SafeCommandRx::FileSessionError {
                session_id,
                error: e.to_string(),
            },
            status: -1,
        },
    }
}

pub(super) async fn close_session(
    id: i32,
    handle: &FileBrowserHandle,
    session_id: String,
) -> SafeCommandResponse {
    handle.close_session(session_id.clone()).await;
    SafeCommandResponse {
        id,
        command: SafeCommandRx::FileSessionStopped { session_id },
        status: 0,
    }
}
