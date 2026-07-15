use crate::device::DeviceCommandResponse;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;

/// A reusable, named bundle of commands. Users save a recipe once and can then
/// replay the same set of commands against any device(s) without re-entering them.
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct CommandRecipe {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    #[schema(value_type = Vec<Object>)]
    pub commands: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One command queued when issuing a bundle, identifying the row in
/// `command_queue` so the caller can track its result without guessing.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct QueuedCommand {
    pub device: i32,
    pub cmd_id: i32,
}

/// Returned by `POST /commands/bundles`: the bundle `uuid` and the id of every
/// command it just queued, so results can be polled precisely.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BundleReceipt {
    #[schema(value_type = String)]
    pub uuid: Uuid,
    pub commands: Vec<QueuedCommand>,
}

/// A single bundle with the current state of all its commands.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BundleWithCommands {
    #[schema(value_type = String)]
    pub uuid: Uuid,
    pub created_on: DateTime<Utc>,
    pub user_email: Option<String>,
    pub responses: Vec<DeviceCommandResponse>,
}
