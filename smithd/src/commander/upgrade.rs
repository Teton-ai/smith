use crate::updater::UpdaterHandle;
use crate::utils::schema::{SafeCommandResponse, SafeCommandRx};

pub(super) async fn upgrade(id: i32, updater_handle: &UpdaterHandle) -> SafeCommandResponse {
    updater_handle.check_for_updates().await;
    updater_handle.upgrade_device().await;
    SafeCommandResponse {
        id,
        command: SafeCommandRx::Upgraded,
        status: 0,
    }
}
