use models::device::DeviceCommandResponse;
use sentry::types::Uuid;
use serde::{Deserialize, Serialize};
use smith::utils::schema::SafeCommandRequest;
use sqlx::types::chrono;

pub mod route;

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BundleWithCommands {
    #[schema(value_type = String)]
    pub uuid: Uuid,
    pub created_on: chrono::DateTime<chrono::Utc>,
    pub responses: Vec<DeviceCommandResponse>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct BundleWithCommandsPaginated {
    pub bundles: Vec<BundleWithCommands>,
    pub next: Option<String>,
    pub previous: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct BundleCommands {
    pub devices: Vec<i32>,
    #[schema(value_type = Vec<Object>)]
    pub commands: Vec<SafeCommandRequest>,
}

/// One queued command produced when issuing a bundle, identifying the row in
/// `command_queue` so the caller can track its result without guessing.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct QueuedCommand {
    pub device: i32,
    pub cmd_id: i32,
}

/// Returned by `POST /commands/bundles`. Gives the caller the bundle `uuid` and
/// the id of every command it just queued, so results can be polled precisely.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct BundleReceipt {
    #[schema(value_type = String)]
    pub uuid: Uuid,
    pub commands: Vec<QueuedCommand>,
}

/// A reusable, named bundle of commands. Users save a recipe once and can then
/// replay the same set of commands against any device(s) without re-entering them.
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct CommandRecipe {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    #[schema(value_type = Vec<Object>)]
    pub commands: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Request body for both creating and updating a recipe.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RecipeInput {
    pub name: String,
    pub description: Option<String>,
    #[schema(value_type = Vec<Object>)]
    pub commands: Vec<SafeCommandRequest>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct BundleWithRawResponsesExplicit {
    pub uuid: Uuid,
    pub created_on: chrono::DateTime<chrono::Utc>,
    pub device: i32,
    pub serial_number: String,
    pub cmd_id: i32,
    pub issued_at: chrono::DateTime<chrono::Utc>,
    pub cmd_data: serde_json::Value,
    pub cancelled: bool,
    pub fetched: bool,
    pub fetched_at: Option<chrono::DateTime<chrono::Utc>>,
    pub response_id: Option<i32>,
    pub response_at: Option<chrono::DateTime<chrono::Utc>>,
    pub response: Option<serde_json::Value>,
    pub status: Option<i32>,
}
