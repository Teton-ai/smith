use crate::user::CurrentUser;
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use tracing::info;

pub fn check(current_user: CurrentUser, resource: &str, action: &str) -> bool {
    let has_permission = current_user.has_permission(resource, action);
    info!(
        "{} [{}] [{}] : {}",
        current_user.user_id,
        action,
        resource,
        if has_permission {
            "OK"
        } else {
            "NOT AUTHORIZED"
        }
    );
    has_permission
}

#[derive(Debug, Deserialize)]
pub struct AuthorizationConfig {
    pub roles: HashMap<String, Role>,
}

#[derive(Debug, Deserialize)]
pub struct Role {
    pub description: String,
    pub inherits: Vec<String>,
    pub permissions: Vec<Permission>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Permission {
    pub action: String,
    pub resource: String,
}

impl AuthorizationConfig {
    pub fn new(config: &str) -> Result<Self> {
        let config: AuthorizationConfig = toml::from_str(config)?;
        Ok(config)
    }
}

impl std::fmt::Display for AuthorizationConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "AUTHORIZATION CONFIGURATION")?;
        writeln!(f, "==========================")?;

        if self.roles.is_empty() {
            return writeln!(f, "No roles defined.");
        }

        for (role_name, role) in &self.roles {
            writeln!(f, "\nROLE: {}", role_name)?;
            writeln!(f, "  Description: {}", role.description)?;

            // Print inherited roles
            if role.inherits.is_empty() {
                writeln!(f, "  Inherits: None")?;
            } else {
                writeln!(f, "  Inherits:")?;
                for inherited in &role.inherits {
                    writeln!(f, "    - {}", inherited)?;
                }
            }

            // Print permissions
            if role.permissions.is_empty() {
                writeln!(f, "  Permissions: None")?;
            } else {
                writeln!(f, "  Permissions:")?;

                // Calculate max action length for this role's permissions for alignment
                let max_action_length = role
                    .permissions
                    .iter()
                    .map(|p| p.action.len())
                    .max()
                    .unwrap_or(0);

                for permission in &role.permissions {
                    writeln!(
                        f,
                        "    - {:<width$} on {}",
                        permission.action,
                        permission.resource,
                        width = max_action_length
                    )?;
                }
            }
        }

        Ok(())
    }
}
