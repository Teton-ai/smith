use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::AsyncWriteExt; // for write_all()
use tracing::info;

#[derive(Serialize, Deserialize, Debug)]
pub struct MagicFile {
    pub meta: ConfigMeta,
    pub tunnel: Option<ConfigTunnel>,
    pub scheduler: Option<ConfigScheduler>,
    #[serde(rename = "check")]
    pub checks: Option<Vec<ConfigCheck>>,
    #[serde(rename = "metric")]
    pub metrics: Option<Vec<ConfigMetric>>,
    #[serde(rename = "package")]
    pub packages: Option<Vec<ConfigPackage>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ConfigMeta {
    pub magic_version: i32,
    pub server: String,
    pub release_id: Option<i32>,
    pub target_release_id: Option<i32>,
    pub token: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConfigCheck {
    pub name: String,
    pub cmd: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConfigMetric {
    pub log_only: bool,
    pub name: String,
    pub cmd: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Hash, Eq, PartialEq, Clone)]
pub struct ConfigPackage {
    pub name: String,
    pub version: String,
    pub file: String,
}

impl ConfigPackage {
    // TODO: use this function more
    pub async fn get_system_version(&self) -> Result<String> {
        let name = &self.name;
        let output = tokio::process::Command::new("dpkg")
            .arg("-l")
            .arg(name)
            .output()
            .await
            .unwrap();

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();
        let package_info = lines.get(5).with_context(|| "Failed to get package info")?;
        let fields: Vec<&str> = package_info.split_whitespace().collect();
        let version = fields
            .get(2)
            .with_context(|| "Failed to get package version")?;

        Ok(version.to_string())
    }
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConfigTunnel {
    pub server: String,
    pub secret: String,
}

impl Default for ConfigTunnel {
    fn default() -> Self {
        Self {
            server: "bore.pub".to_string(),
            secret: String::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ConfigScheduler {
    pub app: Vec<String>,
}

impl MagicFile {
    pub fn autoload() -> Result<(Self, Option<PathBuf>)> {
        // check if a magic.toml exists in the current directory
        // if it does call load with the path location
        let magic_in_cwd = std::path::Path::new("./magic.toml");
        let magic_in_etc = std::path::Path::new("/etc/smith/magic.toml");

        if magic_in_cwd.exists() {
            info!("Loading magic.toml: LOCAL");
            // unwrapping here I think its safe, because we construct the path ourselves
            Self::load_from_path(magic_in_cwd.to_str().unwrap())
        } else if magic_in_etc.exists() {
            info!("Loading magic.toml: ETC");
            Self::load_from_path(magic_in_etc.to_str().unwrap())
        } else {
            info!("Found no magic.toml, creating a default one");
            let string = toml::to_string_pretty(&Self {
                meta: ConfigMeta {
                    magic_version: 2,
                    server: "http://127.0.0.1:8080/smith".to_string(),
                    release_id: None,
                    target_release_id: None,
                    token: None,
                },
                tunnel: None,
                scheduler: None,
                checks: None,
                metrics: None,
                packages: None,
            })?;
            std::fs::write(magic_in_cwd, string)?;
            Self::load_from_path(magic_in_cwd.to_str().unwrap())
        }
    }

    pub fn load(location: Option<String>) -> Result<(Self, Option<PathBuf>)> {
        if let Some(location) = location {
            info!("Loading magic.toml: {}", location);
            Self::load_from_path(&location)
        } else {
            Self::autoload()
        }
    }

    pub fn load_from_path(location: &str) -> Result<(Self, Option<PathBuf>)> {
        let contents = std::fs::read_to_string(location).unwrap();
        let magic_file: MagicFile = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse magic file: {}", location))?;

        Ok((magic_file, Some(PathBuf::from(location))))
    }

    pub async fn write_to_file(&self, path: &str) -> Result<()> {
        let string = toml::to_string_pretty(&self)?;
        let mut file = File::create(path).await?;
        file.write_all(string.as_bytes()).await?;
        info!("Wrote magic file to: {}", path);
        Ok(())
    }

    pub fn get_checks(&self) -> Vec<ConfigCheck> {
        self.checks.clone().unwrap_or_default()
    }

    pub fn get_tunnel_details(&self) -> ConfigTunnel {
        match &self.tunnel {
            Some(tunnel) => tunnel.clone(),
            None => ConfigTunnel::default(),
        }
    }

    pub fn get_packages(&self) -> Vec<ConfigPackage> {
        self.packages.clone().unwrap_or_default()
    }

    pub fn set_packages(&mut self, packages: Vec<ConfigPackage>) {
        self.packages = Some(packages);
    }

    pub fn get_server(&self) -> String {
        self.meta.server.clone()
    }

    pub fn get_release_id(&self) -> Option<i32> {
        self.meta.release_id
    }

    pub fn set_release_id(&mut self, release_id: Option<i32>) {
        self.meta.release_id = release_id;
    }

    pub fn get_target_release_id(&self) -> Option<i32> {
        self.meta.target_release_id
    }

    pub fn set_target_release_id(&mut self, target_release_id: Option<i32>) {
        self.meta.target_release_id = target_release_id;
    }

    pub fn get_token(&self) -> Option<String> {
        self.meta.token.clone()
    }

    pub fn set_token(&mut self, token: Option<String>) {
        self.meta.token = token;
    }
}

impl Default for MagicFile {
    fn default() -> Self {
        let default_magic = r#"
[meta]
magic_version = 2
server 		  = "https://api.smith.teton.ai/smith"
"#;
        // this unwrap is safe because we know the default_magic is valid toml
        toml::from_str(default_magic).unwrap()
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn it_works() {
        // test that we can load the default magic file
        super::MagicFile::autoload().unwrap();
    }
}
