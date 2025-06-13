use crate::tunnel::TunnelHandle;
use crate::utils::schema::{SafeCommandResponse, SafeCommandRx};

pub(super) async fn open_port(
    id: i32,
    tunnel_handle: &TunnelHandle,
    port: Option<u16>,
    user: Option<String>,
    pub_key: Option<String>,
) -> SafeCommandResponse {
    let remote_port = tunnel_handle.start_tunnel(port, user, pub_key).await;
    let status = if remote_port > 0 { 0 } else { -1 };

    SafeCommandResponse {
        id,
        command: SafeCommandRx::OpenTunnel {
            port_server: remote_port,
        },
        status,
    }
}

pub(super) async fn close_ssh(id: i32, tunnel_handle: &TunnelHandle) -> SafeCommandResponse {
    tunnel_handle.stop_ssh_tunnel().await;

    SafeCommandResponse {
        id,
        command: SafeCommandRx::TunnelClosed,
        status: 0,
    }
}
