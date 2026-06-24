use crate::State;
use crate::middlewares::authorization;
use crate::user::CurrentUser;
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::Serialize;
use tracing::error;
use utoipa::ToSchema;

const TAG: &str = "users";

/// A user together with every role assigned to them.
#[derive(Serialize, ToSchema)]
pub struct UserWithRoles {
    pub id: i32,
    pub email: Option<String>,
    pub roles: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[utoipa::path(
    get,
    path = "/users",
    responses(
        (status = 200, description = "List of users retrieved successfully", body = Vec<UserWithRoles>),
        (status = 403, description = "Forbidden"),
        (status = 500, description = "Failed to retrieve users"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = TAG
)]
pub async fn get_users(
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
) -> Result<Json<Vec<UserWithRoles>>, StatusCode> {
    if !authorization::check(current_user, "users", "read") {
        return Err(StatusCode::FORBIDDEN);
    }

    let users = sqlx::query_as!(
        UserWithRoles,
        r#"
            SELECT
                u.id,
                u.email,
                u.created_at AS "created_at!",
                u.updated_at AS "updated_at!",
                COALESCE(
                    ARRAY_AGG(ur.role ORDER BY ur.role) FILTER (WHERE ur.role IS NOT NULL),
                    '{}'
                ) AS "roles!"
            FROM auth.users u
            LEFT JOIN auth.users_roles ur ON ur.user_id = u.id
            GROUP BY u.id
            ORDER BY u.created_at DESC
        "#
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("error: failed to get users: {:?}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(users))
}

/// A single { action, resource } permission, flattened for the dashboard.
#[derive(Serialize, ToSchema)]
pub struct PermissionInfo {
    pub action: String,
    pub resource: String,
}

/// A role definition with both its directly declared permissions and the full
/// effective set (its own plus everything resolved transitively via `inherits`).
#[derive(Serialize, ToSchema)]
pub struct RoleInfo {
    pub name: String,
    pub description: String,
    pub inherits: Vec<String>,
    pub permissions: Vec<PermissionInfo>,
    pub effective_permissions: Vec<PermissionInfo>,
}

#[utoipa::path(
    get,
    path = "/roles",
    responses(
        (status = 200, description = "List of roles retrieved successfully", body = Vec<RoleInfo>),
        (status = 403, description = "Forbidden"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = TAG
)]
pub async fn get_roles(
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
) -> Result<Json<Vec<RoleInfo>>, StatusCode> {
    if !authorization::check(current_user, "users", "read") {
        return Err(StatusCode::FORBIDDEN);
    }

    let mut roles: Vec<RoleInfo> = state
        .authorization
        .roles
        .iter()
        .map(|(name, role)| {
            let permissions = role
                .permissions
                .iter()
                .map(|p| PermissionInfo {
                    action: p.action.clone(),
                    resource: p.resource.clone(),
                })
                .collect();

            let effective_permissions = state
                .authorization
                .permissions_for_role(name)
                .into_iter()
                .map(|p| PermissionInfo {
                    action: p.action,
                    resource: p.resource,
                })
                .collect();

            RoleInfo {
                name: name.clone(),
                description: role.description.clone(),
                inherits: role.inherits.clone(),
                permissions,
                effective_permissions,
            }
        })
        .collect();

    // The config is a HashMap, so sort for a stable order in the UI.
    roles.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(Json(roles))
}
