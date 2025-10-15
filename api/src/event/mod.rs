pub mod route;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub enum PublicEvent {
    ApprovedDevice { serial_number: String },
}
