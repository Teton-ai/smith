use anyhow::Result;
use models::{
    deployment::{Deployment, DeploymentRequest},
    device::{CommandsPaginated, Device, DeviceCommandResponse, DeviceFilter},
    distribution::{Distribution, NewDistributionRelease},
    release::{Release, UpdateRelease},
};
use reqwest::{Client, Response};
use smith::utils::schema::{self, Package};
use std::collections::HashMap;

trait HandleApiError: Sized {
    async fn handle_error(self) -> anyhow::Result<Self>;
}

impl HandleApiError for Response {
    async fn handle_error(self) -> anyhow::Result<Self> {
        let status = self.status();
        if status.is_client_error() || status.is_server_error() {
            let text = self.text().await?;
            Err(anyhow::anyhow!("Api Error {}. {}", status.as_str(), text))
        } else {
            Ok(self)
        }
    }
}

pub struct SmithAPI {
    domain: String,
    bearer_token: String,
}

impl SmithAPI {
    pub fn new(secrets: crate::auth::SessionSecrets, config: &crate::config::Config) -> Self {
        let domain = config.current_domain();

        let bearer_token = secrets
            .bearer_token(&config.current_profile)
            .expect("A bearer token is expected");

        Self {
            domain,
            bearer_token,
        }
    }

    pub async fn get_release_packages(&self, release_id: i32) -> Result<Vec<Package>> {
        let client = Client::new();

        let resp = client
            .get(format!("{}/releases/{}/packages", self.domain, release_id))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .send()
            .await?
            .error_for_status()?;

        let packages = resp.json().await?;

        Ok(packages)
    }

    pub async fn create_distribution_release(
        &self,
        distribution_id: i32,
        request: NewDistributionRelease,
    ) -> Result<i32> {
        let client = Client::new();

        let resp = client
            .post(format!(
                "{}/distributions/{}/releases",
                self.domain, distribution_id
            ))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .json(&request)
            .send()
            .await?
            .error_for_status()?;

        let new_release_id = resp.json().await?;

        Ok(new_release_id)
    }

    pub async fn get_devices(&self, query: DeviceFilter) -> Result<Vec<Device>> {
        let client = Client::new();

        let resp = client
            .get(format!(
                "{}/devices?{}",
                self.domain,
                serde_html_form::to_string(&query)?
            ))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .send()
            .await?;

        let devices = resp.json().await?;

        Ok(devices)
    }

    pub async fn get_device(&self, device_id_or_serial_number: String) -> Result<Device> {
        let client = Client::new();

        let resp = client
            .get(format!(
                "{}/devices/{}",
                self.domain, device_id_or_serial_number
            ))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .send()
            .await?;

        let device = resp.json().await?;

        Ok(device)
    }

    pub async fn get_releases(&self) -> Result<Vec<Release>> {
        let client = Client::new();

        let resp = client
            .get(format!("{}/releases", self.domain))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .send();

        Ok(resp.await?.error_for_status()?.json().await?)
    }

    pub async fn get_distribution_releases(&self, distribution_id: i32) -> Result<Vec<Release>> {
        let client = Client::new();

        let resp = client
            .get(format!(
                "{}/distributions/{}/releases",
                self.domain, distribution_id
            ))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .send();

        Ok(resp.await?.error_for_status()?.json().await?)
    }

    pub async fn get_latest_distribution_release(&self, distribution_id: i32) -> Result<Release> {
        let client = Client::new();

        let resp = client
            .get(format!(
                "{}/distributions/{}/releases/latest",
                self.domain, distribution_id
            ))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .send();

        Ok(resp.await?.error_for_status()?.json().await?)
    }

    pub async fn get_release_info(&self, release_id: String) -> Result<Release> {
        let client = Client::new();

        let resp = client
            .get(format!("{}/releases/{}", self.domain, release_id))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .send();

        Ok(resp.await?.error_for_status()?.json().await?)
    }

    pub async fn update_release(
        &self,
        release_id: i32,
        update_release: UpdateRelease,
    ) -> Result<()> {
        let client = Client::new();

        client
            .post(format!("{}/releases/{}", self.domain, release_id))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .json(&update_release)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    pub async fn deploy_release(
        &self,
        release_id: String,
        deployment_request: Option<DeploymentRequest>,
    ) -> Result<Deployment> {
        let client = Client::new();

        let resp = client
            .post(format!(
                "{}/releases/{}/deployment",
                self.domain, release_id
            ))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .json(&deployment_request)
            .send()
            .await?
            .handle_error()
            .await?;

        let deployment = resp.json().await?;

        Ok(deployment)
    }

    pub async fn deploy_release_check_done(&self, release_id: String) -> Result<Deployment> {
        let client = Client::new();

        let resp = client
            .patch(format!(
                "{}/releases/{}/deployment",
                self.domain, release_id
            ))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .send();

        let deployment = resp.await?.error_for_status()?.json().await?;

        Ok(deployment)
    }

    pub async fn get_distributions(&self) -> Result<Vec<Distribution>> {
        let client = Client::new();

        let resp = client
            .get(format!("{}/distributions", self.domain))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .send()
            .await?
            .error_for_status()?;

        let distros = resp.json().await?;

        Ok(distros)
    }

    pub async fn open_tunnel(&self, device_id: u64, pub_key: String, user: String) -> Result<()> {
        let client = Client::new();

        let open_tunnel_command = schema::SafeCommandRequest {
            id: 0,
            command: schema::SafeCommandTx::OpenTunnel {
                port: None,
                pub_key: Some(pub_key),
                user: Some(user),
            },
            continue_on_error: false,
        };

        let resp = client
            .post(format!("{}/devices/{device_id}/commands", self.domain))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .json(&serde_json::json!([open_tunnel_command]));

        let resp = resp.send();

        // check if return code was 201
        let response_code = resp.await?.status();

        if response_code != 201 {
            return Err(anyhow::anyhow!("Failed to open tunnel"));
        }

        Ok(())
    }

    pub async fn get_last_command(&self, device_id: u64) -> Result<DeviceCommandResponse> {
        let client = Client::new();

        let resp = client
            .get(format!("{}/devices/{device_id}/commands", self.domain))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .send()
            .await?
            .error_for_status()?;

        let commands: CommandsPaginated = resp.json().await?;

        Ok(commands
            .commands
            .first()
            .ok_or_else(|| anyhow::anyhow!("No commands found for device {device_id}"))?
            .clone())
    }

    pub async fn test_network(&self, device: String) -> Result<()> {
        let client = Client::new();

        let test_network_command = schema::SafeCommandRequest {
            id: 0,
            command: schema::SafeCommandTx::TestNetwork,
            continue_on_error: false,
        };

        let resp = client
            .post(format!("{}/devices/{}/commands", self.domain, device))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .json(&serde_json::json!([test_network_command]));

        let response = resp.send().await?;
        let response_code = response.status();

        if response_code != 201 {
            return Err(anyhow::anyhow!(
                "Failed to send network test command: {}",
                response_code
            ));
        }

        Ok(())
    }

    pub async fn send_logs_command(&self, device_id: u64) -> Result<(u64, u64)> {
        let client = Client::new();

        let logs_command = schema::SafeCommandRequest {
            id: 0,
            command: schema::SafeCommandTx::FreeForm {
                cmd: String::from("journalctl -r -n 500"),
            },
            continue_on_error: false,
        };

        let resp = client
            .post(format!("{}/devices/{device_id}/commands", self.domain))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .json(&serde_json::json!([logs_command]))
            .send();

        resp.await?.error_for_status()?;

        // Brief delay to let the API process the command
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // The API returns 201 with empty body, so we need to fetch the last command to get the ID
        let last_command = self.get_last_command(device_id).await?;

        Ok((device_id, last_command.cmd_id as u64))
    }

    pub async fn send_service_status_command(
        &self,
        device_id: u64,
        unit: String,
    ) -> Result<(u64, u64)> {
        let client = Client::new();

        let service_command = schema::SafeCommandRequest {
            id: 0,
            command: schema::SafeCommandTx::FreeForm {
                cmd: format!("systemctl status {}", unit),
            },
            continue_on_error: false,
        };

        let resp = client
            .post(format!("{}/devices/{device_id}/commands", self.domain))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .json(&serde_json::json!([service_command]))
            .send();

        resp.await?.error_for_status()?;

        // Brief delay to let the API process the command
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // The API returns 201 with empty body, so we need to fetch the last command to get the ID
        let last_command = self.get_last_command(device_id).await?;

        Ok((device_id, last_command.cmd_id as u64))
    }

    pub async fn send_smithd_status_command(&self, device_id: u64) -> Result<(u64, u64)> {
        let client = Client::new();

        let status_command = schema::SafeCommandRequest {
            id: 0,
            command: schema::SafeCommandTx::FreeForm {
                cmd: String::from("smithd status"),
            },
            continue_on_error: false,
        };

        let resp = client
            .post(format!("{}/devices/{device_id}/commands", self.domain))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .json(&serde_json::json!([status_command]))
            .send();

        resp.await?.error_for_status()?;

        // Brief delay to let the API process the command
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // The API returns 201 with empty body, so we need to fetch the last command to get the ID
        let last_command = self.get_last_command(device_id).await?;

        Ok((device_id, last_command.cmd_id as u64))
    }

    pub async fn get_device_command(
        &self,
        device_id: u64,
        command_id: u64,
    ) -> Result<DeviceCommandResponse> {
        let client = Client::new();

        // Get commands for device and filter to find the specific one
        let resp = client
            .get(format!(
                "{}/devices/{}/commands?limit=500",
                self.domain, device_id
            ))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .send();

        let response: CommandsPaginated = resp.await?.error_for_status()?.json().await?;

        let command = response
            .commands
            .iter()
            .find(|cmd| cmd.cmd_id as u64 == command_id)
            .ok_or_else(|| {
                anyhow::anyhow!("Command {} not found for device {}", command_id, device_id)
            })?;

        Ok(command.clone())
    }

    pub async fn send_custom_command(&self, device_id: u64, cmd: String) -> Result<(u64, u64)> {
        let client = Client::new();

        let custom_command = schema::SafeCommandRequest {
            id: 0,
            command: schema::SafeCommandTx::FreeForm { cmd },
            continue_on_error: false,
        };

        let resp = client
            .post(format!("{}/devices/{device_id}/commands", self.domain))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .json(&serde_json::json!([custom_command]))
            .send();

        resp.await?.error_for_status()?;

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let last_command = self.get_last_command(device_id).await?;

        Ok((device_id, last_command.cmd_id as u64))
    }

    pub async fn update_device_labels(
        &self,
        device_id: u64,
        labels: HashMap<String, String>,
    ) -> Result<()> {
        let client = Client::new();

        let resp = client
            .patch(format!("{}/devices/{}", self.domain, device_id))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .json(&serde_json::json!({ "labels": labels }))
            .send();

        resp.await?.error_for_status()?;

        Ok(())
    }
}
