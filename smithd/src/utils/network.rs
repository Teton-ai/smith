use crate::{downloader::DownloaderHandle, magic::structure::ConfigPackage};
use anyhow::{Context, Result};
use flate2::{Compression, write::GzEncoder};
use reqwest::{Response, StatusCode};
use std::{env, io::Write, time::Duration};
use tracing::{error, info, warn};

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager"
)]
trait NetworkManager {
    fn get_device_by_ip_iface(&self, iface: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.Device.Wireless",
    default_service = "org.freedesktop.NetworkManager"
)]
trait WirelessDevice {
    #[zbus(property, name = "PermHwAddress")]
    fn permanent_hw_address(&self) -> zbus::Result<String>;
}

fn validate_permanent_mac(value: &str) -> Result<String> {
    let mac: pnet::util::MacAddr = value.parse().context("Invalid permanent Wi-Fi MAC")?;
    anyhow::ensure!(
        !mac.is_zero() && mac.is_unicast(),
        "Invalid permanent Wi-Fi MAC"
    );
    Ok(mac.to_string())
}

/// Never fall back to the active address: while disconnected it can be a scan MAC.
pub(crate) async fn permanent_wifi_mac(interface: &str) -> Result<String> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let connection = zbus::Connection::system().await?;
        permanent_wifi_mac_on(&connection, interface).await
    })
    .await
    .context("Timed out reading permanent Wi-Fi MAC")?
}

async fn permanent_wifi_mac_on(connection: &zbus::Connection, interface: &str) -> Result<String> {
    let manager = NetworkManagerProxy::new(connection).await?;
    let path = manager.get_device_by_ip_iface(interface).await?;
    let device = WirelessDeviceProxy::builder(connection)
        .path(path)?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await?;
    validate_permanent_mac(&device.permanent_hw_address().await?)
}

pub struct NetworkClient {
    hostname: String,
    id: String,
    client: reqwest::Client,
}

impl Default for NetworkClient {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkClient {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .gzip(true)
            .build()
            .unwrap();

        let id = crate::utils::system::get_serial_number();

        let hostname = "".to_owned();

        Self {
            id,
            hostname,
            client,
        }
    }

    pub fn get_serial(&self) -> String {
        self.id.clone()
    }

    pub async fn get_wifi_mac(&self) -> String {
        let interface = crate::settings::driver::wifi_interface(std::path::Path::new(
            crate::settings::driver::SYS_CLASS_NET,
        ))
        .await;
        let Some(interface) = interface else {
            warn!("No physical Wi-Fi interface found for registration");
            return "DE:MO:00:00:00:00".to_owned();
        };
        match permanent_wifi_mac(&interface.name).await {
            Ok(mac) => mac,
            Err(e) => {
                warn!("Failed to read permanent Wi-Fi MAC: {e:#}");
                "DE:MO:00:00:00:00".to_owned()
            }
        }
    }

    pub fn set_hostname(&mut self, hostname: String) {
        self.hostname = hostname;
    }

    pub async fn send_compressed_post<T: serde::Serialize>(
        &self,
        token: &str,
        endpoint: &str,
        message: &T,
    ) -> Result<(StatusCode, Response)> {
        let client = self.client.clone();
        let url = format!("{}{}", self.hostname, endpoint);

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        let json = serde_json::to_vec(&message).unwrap_or_default();
        encoder.write_all(&json)?;

        let compressed_data = encoder.finish()?;

        let request = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .header("Content-Encoding", "gzip")
            .body(compressed_data)
            .send()
            .await?;

        let status_code = request.status();

        Ok((status_code, request))
    }

    pub async fn get_release_packages(
        &self,
        release_id: i32,
        token: &str,
    ) -> Result<Vec<ConfigPackage>> {
        let url = format!("{}/releases/{}/packages", self.hostname, release_id);
        let response = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await;

        response?
            .json()
            .await
            .with_context(|| "Failed to Parse JSON respone")
    }

    async fn validate_package_file(path: &std::path::Path) -> Result<bool> {
        // Check if file exists and has content
        let metadata = match tokio::fs::metadata(path).await {
            Ok(m) => m,
            Err(_) => return Ok(false),
        };

        // Only reject obviously broken 0-byte files
        // Files without etag may still be valid (downloaded before .part implementation)
        Ok(metadata.len() > 0)
    }

    pub async fn get_package(
        &self,
        package_name: &str,
        downloader: &DownloaderHandle,
    ) -> Result<()> {
        let path = env::current_dir()?;

        let mut local_packages_folder = path.clone();
        local_packages_folder.push("packages");

        let mut local_package_path = local_packages_folder.clone();
        local_package_path.push(package_name);

        if local_package_path.exists() {
            // Validate existing file
            if Self::validate_package_file(&local_package_path).await? {
                info!(
                    "Package {} already exists and is valid, using as-is (no download needed)",
                    package_name
                );
                return Ok(());
            } else {
                warn!(
                    "Package file {} exists but is 0 bytes (broken download), removing for re-download",
                    package_name
                );
                tokio::fs::remove_file(&local_package_path)
                    .await
                    .inspect_err(|e| error!("Failed to remove invalid package file: {}", e))?;
                info!("Removed 0-byte package, proceeding with download");
            }
        }

        // File doesn't exist or was invalid and removed - proceed with download
        info!("Fetching package: {}", package_name);

        let remote_file = format!("packages/{}", package_name);
        downloader
            // steady download at 2MB/s lets play nice in these networks
            .download_blocking(&remote_file, local_package_path.to_str().unwrap_or(""), 2.0)
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod mac_tests {
    use super::validate_permanent_mac;

    struct MockManager;

    #[zbus::interface(name = "org.freedesktop.NetworkManager")]
    impl MockManager {
        fn get_device_by_ip_iface(
            &self,
            iface: &str,
        ) -> zbus::fdo::Result<zbus::zvariant::OwnedObjectPath> {
            if iface != "wlan0" {
                return Err(zbus::fdo::Error::Failed("Unknown device".to_owned()));
            }
            zbus::zvariant::OwnedObjectPath::try_from("/org/freedesktop/NetworkManager/Devices/1")
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
        }
    }

    struct MockWireless;

    #[zbus::interface(name = "org.freedesktop.NetworkManager.Device.Wireless")]
    impl MockWireless {
        #[zbus(property, name = "PermHwAddress")]
        fn permanent_hw_address(&self) -> &str {
            "00:11:22:33:44:55"
        }

        // A disconnected device's randomized scan address must never be read.
        #[zbus(property, name = "HwAddress")]
        fn hw_address(&self) -> &str {
            "02:aa:bb:cc:dd:ee"
        }
    }

    #[tokio::test]
    async fn reports_permanent_address_when_active_address_is_randomized()
    -> Result<(), anyhow::Error> {
        let (server_socket, client_socket) = std::os::unix::net::UnixStream::pair()?;
        let server = zbus::connection::Builder::async_io_unix_stream(server_socket)
            .server(zbus::Guid::generate())?
            .p2p()
            .serve_at("/org/freedesktop/NetworkManager", MockManager)?
            .serve_at("/org/freedesktop/NetworkManager/Devices/1", MockWireless)?
            .build();
        let client = zbus::connection::Builder::async_io_unix_stream(client_socket)
            .p2p()
            .build();
        let (_server, client) = tokio::try_join!(server, client)?;
        assert_eq!(
            super::permanent_wifi_mac_on(&client, "wlan0").await?,
            "00:11:22:33:44:55"
        );
        assert!(
            super::permanent_wifi_mac_on(&client, "missing")
                .await
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn accepts_hardware_addresses_including_locally_administered() -> Result<(), anyhow::Error> {
        assert_eq!(
            validate_permanent_mac("AA:BB:CC:DD:EE:FF")?,
            "aa:bb:cc:dd:ee:ff"
        );
        assert_eq!(
            validate_permanent_mac("00:11:22:33:44:55")?,
            "00:11:22:33:44:55"
        );
        Ok(())
    }

    #[test]
    fn rejects_missing_or_invalid_permanent_addresses() {
        for value in [
            "",
            "invalid",
            "00:00:00:00:00:00",
            "ff:ff:ff:ff:ff:ff",
            "01:11:22:33:44:55",
        ] {
            assert!(validate_permanent_mac(value).is_err(), "{value}");
        }
    }
}
