use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use smith::utils::schema;

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

    pub async fn get_devices(&self, serial_number: Option<String>) -> Result<String> {
        let client = Client::new();

        let resp = client
            .get(format!("{}/devices", self.domain))
            .header("Authorization", format!("Bearer {}", &self.bearer_token))
            .query(&[("serial_number", serial_number)])
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

        let last_command = &commands["commands"][0];

        Ok(last_command.clone())
    }
}
