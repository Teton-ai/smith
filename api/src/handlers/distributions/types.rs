use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Distribution {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub architecture: String,
    pub num_packages: Option<i32>,
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
