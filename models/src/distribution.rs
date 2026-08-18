use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct Distribution {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub architecture: String,
    pub num_packages: Option<i32>,
    pub archived: bool,
    /// What followers converge to. Moving it moves the fleet.
    pub latest_release_id: Option<i32>,
    /// What the production line flashes and new devices are pinned to.
    /// Moving it moves nobody already in the field.
    pub base_release_id: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct NewDistributionRelease {
    pub version: String,
    pub packages: Vec<i32>,
    #[serde(default)]
    pub release_candidate: bool,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SetBaseRelease {
    pub release_id: i32,
    /// Bypass the soak gate. For the first base a distribution ever gets, and
    /// for emergencies -- both are logged.
    #[serde(default)]
    pub force: bool,
    pub reason: Option<String>,
}
