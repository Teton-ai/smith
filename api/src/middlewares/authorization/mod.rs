use crate::user::CurrentUser;
use anyhow::Result;
use serde::Deserialize;
use smith::utils::schema::{SafeCommandRequest, SafeCommandTx};
use std::collections::{HashMap, HashSet};
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

/// The permission required to dispatch a given command. Exhaustive on purpose:
/// a new `SafeCommandTx` variant cannot be added without declaring how it is
/// gated, so dangerous commands can never slip through ungated.
pub fn required_permission(command: &SafeCommandTx) -> Permission {
    use SafeCommandTx::*;
    let action = match command {
        FreeForm { .. } => "freeform",
        OpenTunnel { .. } | CloseTunnel => "tunnel",
        DownloadOTA { .. } | CheckOTAStatus | StartOTA => "ota",
        Ping
        | Upgrade
        | Restart
        | UpdateNetwork { .. }
        | UpdateVariables { .. }
        | TestNetwork
        | ExtendedNetworkTest { .. }
        | StreamLogs { .. }
        | StopLogStream { .. }
        | RunAudit
        | GetLogs { .. }
        | RunNetworkDiagnostic
        | ReportNMProfiles
        | WifiScan => "basic",
    };
    Permission {
        action: action.to_string(),
        resource: "commands".to_string(),
    }
}

/// Returns true only if `current_user` is allowed to dispatch every command in
/// the bundle. The bundle is all-or-nothing: one disallowed command rejects the
/// whole request so partial bundles are never queued.
pub fn authorize_commands(current_user: &CurrentUser, commands: &[SafeCommandRequest]) -> bool {
    commands.iter().all(|req| {
        let permission = required_permission(&req.command);
        let allowed = current_user.has_permission(&permission.resource, &permission.action);
        if !allowed {
            info!(
                "{} [{}] [{}] : NOT AUTHORIZED",
                current_user.user_id, permission.action, permission.resource
            );
        }
        allowed
    })
}

#[derive(Debug, Deserialize)]
pub struct AuthorizationConfig {
    pub roles: HashMap<String, Role>,
}

/// Maps a user's email to the role they should hold. Parsed from the TOML
/// content of the `ACCOUNTS_CONFIG` env var (injected from a version-controlled
/// file at deploy time) and reconciled into the database on startup, so the
/// config — not the database — is the source of truth for who holds an elevated
/// role. Absent `ACCOUNTS_CONFIG`, the feature is off.
#[derive(Debug, Deserialize, Default)]
pub struct AccountsConfig {
    #[serde(default)]
    pub accounts: HashMap<String, String>,
}

impl AccountsConfig {
    pub fn new(config: &str) -> Result<Self> {
        let parsed: AccountsConfig = toml::from_str(config)?;
        // Emails are normalized (lowercased, whitespace-trimmed) when users are
        // created/updated (see `CurrentUser::create`/`update_email`), so normalize
        // the config keys the same way. This keeps reconciliation matching robust
        // and collapses any case-duplicate keys into one entry (last wins).
        let accounts = parsed
            .accounts
            .into_iter()
            .map(|(email, role)| (email.trim().to_lowercase(), role))
            .collect();
        Ok(AccountsConfig { accounts })
    }

    /// (email, role) pairs whose role is not defined in `authorization`. Used at
    /// startup to surface typos: an undefined role grants no permissions, so it
    /// is skipped during reconciliation rather than assigned.
    pub fn unknown_roles<'a>(
        &'a self,
        authorization: &AuthorizationConfig,
    ) -> Vec<(&'a str, &'a str)> {
        self.accounts
            .iter()
            .filter(|(_, role)| !authorization.roles.contains_key(role.as_str()))
            .map(|(email, role)| (email.as_str(), role.as_str()))
            .collect()
    }
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

    /// All permissions granted by a role, including those of the roles it
    /// inherits (resolved transitively). Unknown or cyclic `inherits` entries
    /// are skipped rather than erroring, so a typo can't lock everyone out.
    pub fn permissions_for_role(&self, role_name: &str) -> Vec<Permission> {
        let mut permissions = Vec::new();
        let mut visited = HashSet::new();
        self.collect_permissions(role_name, &mut permissions, &mut visited);
        permissions
    }

    fn collect_permissions(
        &self,
        role_name: &str,
        permissions: &mut Vec<Permission>,
        visited: &mut HashSet<String>,
    ) {
        if !visited.insert(role_name.to_string()) {
            return;
        }
        if let Some(role) = self.roles.get(role_name) {
            permissions.extend(role.permissions.iter().cloned());
            for parent in &role.inherits {
                self.collect_permissions(parent, permissions, visited);
            }
        }
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
