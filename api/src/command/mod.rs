use models::command::BundleWithCommands;
use sentry::types::Uuid;
use serde::{Deserialize, Serialize};
use smith::utils::schema::SafeCommandRequest;
use sqlx::types::chrono;

pub mod route;

pub fn redact_cmd_data(mut cmd: serde_json::Value) -> serde_json::Value {
    if let Some(networks) = cmd
        .get_mut("ApplyNetworks")
        .and_then(|v| v.get_mut("networks"))
        .and_then(|v| v.as_array_mut())
    {
        for network in networks.iter_mut() {
            if let Some(creds) = network.get_mut("credentials")
                && let Some(obj) = creds.as_object_mut()
            {
                obj.remove("psk");
            }
        }
    }
    cmd
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

/// Request body for both creating and updating a recipe.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RecipeInput {
    pub name: String,
    pub description: Option<String>,
    #[schema(value_type = Vec<Object>)]
    pub commands: Vec<SafeCommandRequest>,
}

/// Request body for triggering a recipe: the devices to run it against. The
/// commands come from the stored recipe, not the caller.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TriggerRecipeInput {
    pub devices: Vec<i32>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct BundleWithRawResponsesExplicit {
    pub uuid: Uuid,
    pub created_on: chrono::DateTime<chrono::Utc>,
    pub user_email: Option<String>,
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
