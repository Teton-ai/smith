use serde::{Deserialize, Serialize};
use sqlx::types::chrono;

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Distribution {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub architecture: String,
    pub num_packages: Option<i32>,
}

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
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UpdateRelease {
    pub draft: Option<bool>,
    pub yanked: Option<bool>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewDistribution {
    pub name: String,
    pub description: Option<String>,
    pub architecture: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewDistributionRelease {
    pub version: String,
    pub packages: Vec<i32>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ReplacementPackage {
    pub id: i32,
}
