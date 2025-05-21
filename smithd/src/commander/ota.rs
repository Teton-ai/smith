use crate::downloader::{DownloaderHandle, DownloadingStatus};
use crate::filemanager::FileManagerHandle;
use crate::utils::schema::{SafeCommandResponse, SafeCommandRx};

struct OTAConstants;

impl OTAConstants {
    const TOOLS_STORAGE: &'static str = "/otatool";
    const OTA_STORAGE: &'static str = "/ota";
    const PACKAGE_FILE: &'static str = "ota_payload_package.tar.gz";
    const TOOLS_FILE: &'static str = "ota_tools.tbz2";
    const OTA_SCRIPT_DIR: &'static str =
        "/otatool/Linux_for_Tegra/tools/ota_tools/version_upgrade/";
}

pub(super) async fn download_ota(
    id: i32,
    download_handle: &DownloaderHandle,
    file_handle: &FileManagerHandle,
    tools_file: &str,
    package_file: &str,
    rate: f64,
) -> SafeCommandResponse {
    // Create OTA storage folder
    let arguments: Vec<String> = vec!["-p".to_owned(), OTAConstants::OTA_STORAGE.to_owned()];
    let _ = file_handle // No errors returned from the function
        .execute_system_command("mkdir", arguments, None)
        .await;

    // Create OTA tools storage folder
    let arguments: Vec<String> = vec!["-p".to_owned(), OTAConstants::TOOLS_STORAGE.to_owned()];
    let _ = file_handle
        .execute_system_command("mkdir", arguments, None)
        .await;

    // Download the OTA tools
    let remote_file = format!("ota/{}", tools_file);
    let local_file = format!(
        "{}/{}",
        OTAConstants::TOOLS_STORAGE,
        OTAConstants::TOOLS_FILE
    );
    let _ = download_handle
        .download(remote_file.as_str(), local_file.as_str(), rate)
        .await;

    // Download the OTA payload package
    let remote_file = format!("ota/{}", package_file);
    let local_file = format!(
        "{}/{}",
        OTAConstants::OTA_STORAGE,
        OTAConstants::PACKAGE_FILE
    );
    let _ = download_handle
        .download(remote_file.as_str(), local_file.as_str(), rate)
        .await;

    SafeCommandResponse {
        id,
        command: SafeCommandRx::DownloadOTA,
        status: 0,
    }
}

pub(super) async fn start_ota(id: i32, file_handle: &FileManagerHandle) -> SafeCommandResponse {
    // TODO: Think about adding its own response here?
    // The function will auto restart the device so I don't think we will ever see this
    let local_file = format!(
        "{}/{}",
        OTAConstants::OTA_STORAGE,
        OTAConstants::PACKAGE_FILE
    );
    let arguments: Vec<String> = vec![local_file];
    match file_handle
        .execute_script(
            "nv_ota_start.sh",
            arguments,
            Some(OTAConstants::OTA_SCRIPT_DIR),
        )
        .await
    {
        Ok(_) => {
            let arguments: Vec<String> = Vec::new();
            let _ = file_handle
                .execute_system_command("reboot", arguments, None)
                .await;
        }
        Err(_) => {
            return SafeCommandResponse {
                id,
                command: SafeCommandRx::DownloadOTA,
                status: -1,
            };
        }
    }

    SafeCommandResponse {
        id,
        command: SafeCommandRx::DownloadOTA,
        status: 0,
    }
}

pub(super) async fn check_ota(id: i32, download_handle: &DownloaderHandle) -> SafeCommandResponse {
    let result = download_handle.check_download_status().await;
    let result_unwrapped = match result {
        Ok(result) => result,
        Err(_) => {
            return SafeCommandResponse {
                id,
                command: SafeCommandRx::CheckOTAStatus {
                    status: "Error".to_string(),
                },
                status: -1,
            };
        }
    };

    let status;
    match result_unwrapped {
        DownloadingStatus::Failed => status = "Failed",
        DownloadingStatus::Downloading => status = "Downloading",
        DownloadingStatus::Success => status = "Success",
    }

    SafeCommandResponse {
        id,
        command: SafeCommandRx::CheckOTAStatus {
            status: status.to_string(),
        },
        status: 0,
    }
}
