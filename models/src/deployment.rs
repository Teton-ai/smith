use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(sqlx::Type, Serialize, Deserialize, Debug, Clone, ToSchema, PartialEq)]
#[sqlx(type_name = "deployment_status", rename_all = "snake_case")]
pub enum DeploymentStatus {
    InProgress,
    Failed,
    Canceled,
    Done,
}

impl std::fmt::Display for DeploymentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentStatus::InProgress => write!(f, "InProgress"),
            DeploymentStatus::Failed => write!(f, "Failed"),
            DeploymentStatus::Canceled => write!(f, "Canceled"),
            DeploymentStatus::Done => write!(f, "Done"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct Deployment {
    pub id: i32,
    pub release_id: i32,
    pub status: DeploymentStatus,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Canaries are always picked automatically. A stable release rolls out to the
/// fleet and takes no pilot group; a release candidate needs one, by labels or
/// by devices. A pilot group under 10 devices is deployed to directly.
#[derive(Serialize, Deserialize, Debug, Clone, Default, ToSchema)]
pub struct DeploymentRequest {
    /// A release candidate's pilot group: devices with any of these labels,
    /// e.g. `["env=staging"]`.
    pub pilot_device_labels: Option<Vec<String>>,
    /// A release candidate's pilot group: these device IDs. Cannot be used
    /// together with `pilot_device_labels`.
    pub pilot_device_ids: Option<Vec<i32>>,
}
