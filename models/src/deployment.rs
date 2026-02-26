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

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct DeploymentRequest {
    /// Optionally decide which devices go into the canary release with this filter.
    /// e.g. `canary_device_labels=["label1=a", "label2=b"]`
    pub canary_device_labels: Option<Vec<String>>,
    /// Optionally specify exact device IDs for the canary release.
    /// Cannot be used together with `canary_device_labels`.
    pub canary_device_ids: Option<Vec<i32>>,
}
