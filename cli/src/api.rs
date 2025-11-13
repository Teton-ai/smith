use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use smith::utils::schema;
use std::collections::HashMap;

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

    pub async fn get_devices(
        &self,
        serial_number: Option<String>,
        labels: Option<HashMap<String, String>>,
        online: Option<bool>,
    ) -> Result<String> {
        let client = Client::new();

        let mut query_params: Vec<(&str, String)> = vec![];

        if let Some(sn) = serial_number {
            query_params.push(("serial_number", sn));
        }

        if let Some(labels_map) = labels {
            for (key, value) in labels_map {
                query_params.push(("labels", format!("{}={}", key, value)));
            }
        }

        if let Some(is_online) = online {
            query_params.push(("online", is_online.to_string()));
        }

        let resp = client
            .get(format!("{}/devices", self.domain))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .query(&query_params)
            .send();

        let devices = resp.await?.text().await?;

        Ok(devices)
    }

    pub async fn get_release_info(&self, release_id: String) -> Result<Value> {
        let client = Client::new();

        let resp = client
            .get(format!("{}/releases/{}", self.domain, release_id))
            .header("Authorization", &self.bearer_token)
            .send();

        Ok(resp.await?.error_for_status()?.json().await?)
    }

    pub async fn deploy_release(&self, release_id: String) -> Result<Value> {
        let client = Client::new();

        let resp = client
            .post(format!(
                "{}/releases/{}/deployment",
                self.domain, release_id
            ))
            .header("Authorization", &self.bearer_token)
            .send();

        let deployment = resp.await?.error_for_status()?.json().await?;

        Ok(deployment)
    }

    pub async fn deploy_release_check_done(&self, release_id: String) -> Result<Value> {
        let client = Client::new();

        let resp = client
            .patch(format!(
                "{}/releases/{}/deployment",
                self.domain, release_id
            ))
            .header("Authorization", &self.bearer_token)
            .send();

        let deployment = resp.await?.error_for_status()?.json().await?;

        Ok(deployment)
    }

    pub async fn get_distributions(&self) -> Result<String> {
        let client = Client::new();

        let resp = client
            .get(format!("{}/distributions", self.domain))
            .header("Authorization", &self.bearer_token)
            .send();

        let distros = resp.await?.text().await?;

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

    pub async fn get_last_command(&self, device_id: u64) -> Result<serde_json::Value> {
        let client = Client::new();

        let resp = client
            .get(format!("{}/devices/{device_id}/commands", self.domain))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .send();

        let commands = resp.await?.text().await?;

        let commands: Value = serde_json::from_str(&commands)?;

        let last_command = commands["commands"]
            .as_array()
            .and_then(|arr| arr.first())
            .ok_or_else(|| anyhow::anyhow!("No commands found for device"))?;

        Ok(last_command.clone())
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
        let command_id = last_command["cmd_id"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Failed to get command ID from last command"))?;

        Ok((device_id, command_id))
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
        let command_id = last_command["cmd_id"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Failed to get command ID from last command"))?;

        Ok((device_id, command_id))
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
        let command_id = last_command["cmd_id"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Failed to get command ID from last command"))?;

        Ok((device_id, command_id))
    }

    pub async fn get_device_command(
        &self,
        device_id: u64,
        command_id: u64,
    ) -> Result<serde_json::Value> {
        let client = Client::new();

        // Get commands for device and filter to find the specific one
        let resp = client
            .get(format!(
                "{}/devices/{}/commands?limit=500",
                self.domain, device_id
            ))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .send();

        let response = resp.await?.error_for_status()?.json::<Value>().await?;

        let commands = response["commands"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid response format"))?;

        let command = commands
            .iter()
            .find(|cmd| cmd["cmd_id"].as_u64() == Some(command_id))
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
        let command_id = last_command["cmd_id"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Failed to get command ID from last command"))?;

        Ok((device_id, command_id))
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
