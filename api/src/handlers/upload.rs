use crate::State;
use crate::asset::Asset;
use crate::handlers::DeviceWithToken;
use axum::extract::{Multipart, Path};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::error;

#[derive(Debug, Serialize, Deserialize)]
pub struct UploadResult {
    pub url: String,
}

// TODO: Change to streaming, so we are not saving in memory
#[tracing::instrument]
pub async fn upload_file(
    _device: DeviceWithToken,
    path: Option<Path<String>>,
    Extension(state): Extension<State>,
    mut multipart: Multipart,
) -> Result<Json<UploadResult>, StatusCode> {
    let mut file_name = String::new();
    if let Some(prefix) = path {
        file_name.push_str(&prefix.0);
        file_name.push('/');
    }

    let mut file_data = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .expect("error: failed to get next multipart field")
    {
        if let Some(local_file_name) = field.file_name().map(|s| s.to_string()) {
            file_name.push_str(&local_file_name);
        }
        match field.bytes().await {
            Ok(bytes) => file_data.extend(bytes.clone()),
            _ => return Err(StatusCode::BAD_REQUEST),
        };
    }

    if file_name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    Asset::new(&file_name, &file_data, state.config)
        .await
        .map_err(|err| {
            error!("{:?}", err);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(UploadResult {
        url: format!("s3://{}/{}", &state.config.assets_bucket_name, &file_name),
    }))
}
