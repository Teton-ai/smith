use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Release {
    pub id: i32,
    pub distribution_id: i32,
    pub distribution_architecture: String,
    pub distribution_name: String,
    pub version: String,
    pub draft: bool,
    pub yanked: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub user_id: Option<i32>,
    pub user_email: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UpdateRelease {
    pub draft: Option<bool>,
    pub yanked: Option<bool>,
}
