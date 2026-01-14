use crate::magic::{MagicHandle, structure};
use crate::shutdown::ShutdownHandler;
use reqwest::Client;
use tokio::fs;
use tracing::error;
use walkdir::WalkDir;

pub async fn files_upload(path: &str) -> anyhow::Result<()> {
    let shutdown = ShutdownHandler::new();

    let configuration = MagicHandle::new(shutdown.signals());

    configuration.load(None).await;

    let client = Client::new();
    let server_api_url = configuration.get_server().await;
    let metadata = fs::metadata(path).await?;

    if metadata.is_file() {
        upload_file(path, &client, &server_api_url).await?;
    } else {
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
        {
            let file_path = entry.path();
            upload_file(file_path.to_str().unwrap(), &client, &server_api_url).await?;
        }
    }
    Ok(())
}

pub async fn upload_file(
    file_path: &str,
    client: &Client,
    server_api_url: &str,
) -> anyhow::Result<()> {
    let conf = match structure::MagicFile::load(None) {
        Ok((conf, _path)) => Some(conf),
        Err(err) => {
            error!("Failed to load magic file: {}", err);
            None
        }
    };

    if conf.is_none() {
        error!("Failed to load magic file");
        return Ok(());
    }

    let token = conf.unwrap().get_token().unwrap_or_default();

    let file_name = std::path::Path::new(file_path)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();

    // Get presigned URL from API
    let presigned_url: String = client
        .get(format!("{}/upload/presign", server_api_url))
        .header("Authorization", format!("Bearer {}", token))
        .query(&[("filename", file_name)])
        .send()
        .await?
        .json()
        .await?;

    // Read file content
    let content = tokio::fs::read(file_path).await?;

    // Upload directly to S3 using presigned URL
    let response = client.put(&presigned_url).body(content).send().await?;

    if !response.status().is_success() {
        error!("Failed to upload file {}: {}", file_name, response.status());
    }
    Ok(())
}
