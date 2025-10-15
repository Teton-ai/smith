use serde::{Deserialize, Serialize};

pub mod route;

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeviceAuth {
    pub serial_number: String,
    pub authorized: bool,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DeviceTokenForVerification {
    pub token: String,
}
