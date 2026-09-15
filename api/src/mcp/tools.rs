//! Tools call the existing route handlers rather than the database directly, so
//! permission checks, validation and side effects (Slack notifications, service
//! extraction) stay identical to the dashboard's.

use crate::State;
use crate::command::route as command_route;
use crate::command::{BundleCommands, TriggerRecipeInput};
use crate::deployment::route as deployment_route;
use crate::device::route as device_route;
use crate::distribution::route::{self as distribution_route, DistributionsFilter};
use crate::release::route::{self as release_route, PromoteReleaseRequest};
use crate::user::CurrentUser;
use ::sentry::types::Uuid;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::IntoResponse;
use axum::{Extension as AxumExtension, Json as AxumJson};
use models::command::{BundleReceipt, BundleWithCommands};
use models::deployment::DeploymentRequest;
use models::device::DeviceFilter;
use models::distribution::NewDistributionRelease;
use models::release::{Release, ReleaseFilter, UpdateRelease, VersionBump, next_version};
use rmcp::handler::server::common::Extension;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerInfo};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use smith::utils::schema::{SafeCommandRequest, SafeCommandTx};
use std::time::{Duration, Instant};
use tracing::error;

const INSTRUCTIONS: &str = "Smith manages a fleet of Linux devices. Devices can be referred to by \
numeric id or serial number. Releases belong to a distribution; the usual flow is draft_release, \
publish_release, deploy_release (canary devices first), get_deployment to watch it, then \
confirm_full_rollout. Device commands are queued and run when the device next checks in: tools \
that queue commands return a bundle UUID, and get_command_results fetches the output later.";

/// Long polls hold an HTTP request open; stay under typical load balancer idle timeouts.
const MAX_WAIT_SECONDS: u64 = 45;
const DEFAULT_WAIT_SECONDS: u64 = 20;
const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// Commands fan out to real devices; larger rollouts belong in deployments or recipes.
const MAX_TARGET_DEVICES: usize = 100;

type ToolResult = Result<CallToolResult, CallToolResult>;

#[derive(Clone)]
pub struct SmithMcp {
    state: State,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListDevicesParams {
    /// Only devices with all of these labels, formatted `key=value`.
    #[serde(default)]
    pub labels: Vec<String>,
    /// true for devices seen in the last 3 minutes, false for offline devices.
    pub online: Option<bool>,
    /// true for devices not yet running their target release.
    pub outdated: Option<bool>,
    /// Comma-separated terms matched against serial number, hostname and model.
    pub search: Option<String>,
    /// Only devices currently running this release.
    pub release_id: Option<i32>,
    /// Only devices running a release of this distribution.
    pub distribution_id: Option<i32>,
    /// Page size, default 50, max 1000.
    pub limit: Option<i64>,
    /// Number of devices to skip.
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeviceParams {
    /// Device id or serial number.
    pub device: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DevicesParams {
    /// Device ids or serial numbers.
    pub devices: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListReleasesParams {
    /// Only releases of this distribution.
    pub distribution_id: Option<i32>,
    /// Filter by LTS designation.
    pub lts: Option<bool>,
    /// Most recent releases to return, default 20.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReleaseParams {
    pub release_id: i32,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Bump {
    /// Bug fixes and small changes.
    Patch,
    /// New features, backwards compatible.
    Minor,
    /// Breaking changes.
    Major,
}

impl From<Bump> for VersionBump {
    fn from(bump: Bump) -> Self {
        match bump {
            Bump::Patch => VersionBump::Patch,
            Bump::Minor => VersionBump::Minor,
            Bump::Major => VersionBump::Major,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DraftReleaseParams {
    pub distribution_id: i32,
    /// Which semver component to increment.
    pub bump: Bump,
    /// Create as a release candidate (`-rc` suffix), to be promoted later.
    #[serde(default)]
    pub release_candidate: bool,
    /// Release to copy packages and version from. Defaults to the most recently created release of the distribution.
    pub base_release_id: Option<i32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetLtsParams {
    pub release_id: i32,
    /// true to mark the release LTS, false to clear the designation.
    pub lts: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PromoteParams {
    /// The release candidate to promote.
    pub release_id: i32,
    /// Version of the new, non-RC release, e.g. `1.4.0`.
    pub version: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeployParams {
    pub release_id: i32,
    /// Pick canary devices by label (`key=value`). Cannot be combined with canary_device_ids.
    pub canary_device_labels: Option<Vec<String>>,
    /// Pick exact canary devices by id. Cannot be combined with canary_device_labels.
    pub canary_device_ids: Option<Vec<i32>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LogsParams {
    /// Device id or serial number.
    pub device: String,
    /// systemd unit to filter by, e.g. `smithd`.
    pub unit: Option<String>,
    /// journalctl --since, e.g. `1h ago`.
    pub since: Option<String>,
    /// journalctl --until.
    pub until: Option<String>,
    /// journalctl --grep pattern.
    pub grep: Option<String>,
    /// Seconds to wait for the device to answer, default 20, max 45.
    pub wait_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ServiceParams {
    /// Device ids or serial numbers.
    pub devices: Vec<String>,
    /// systemd unit name, e.g. `nginx.service`.
    pub unit: String,
    /// Seconds to wait for the devices to answer, default 20, max 45.
    pub wait_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CommandResultsParams {
    /// Bundle UUID returned when the commands were queued.
    pub bundle_uuid: String,
    /// Seconds to wait for outstanding results, default 0, max 45.
    pub wait_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TriggerRecipeParams {
    pub recipe_id: i32,
    /// Device ids or serial numbers.
    pub devices: Vec<String>,
}

#[tool_router]
impl SmithMcp {
    pub fn new(state: State) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    /// List devices in the fleet, with their online state, labels and current/target release.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn list_devices(&self, Parameters(params): Parameters<ListDevicesParams>) -> ToolResult {
        let filter = DeviceFilter {
            labels: params.labels,
            online: params.online,
            outdated: params.outdated,
            search: params.search,
            release_id: params.release_id,
            distribution_id: params.distribution_id,
            limit: Some(params.limit.unwrap_or(50)),
            offset: params.offset,
            ..Default::default()
        };
        let (_, AxumJson(devices)) = call(device_route::get_devices(
            AxumExtension(self.state.clone()),
            axum_extra::extract::Query(filter),
        ))
        .await?;

        let summary: Vec<_> = devices
            .iter()
            .map(|device| {
                json!({
                    "id": device.id,
                    "serial_number": device.serial_number,
                    "online": device.online,
                    "last_seen": device.last_seen,
                    "approved": device.approved,
                    "labels": device.labels,
                    "release": device.release.as_ref().map(release_summary),
                    "target_release": device.target_release.as_ref().map(release_summary),
                })
            })
            .collect();
        json_result(&summary)
    }

    /// Full details for one device: system info, network, modem, IP and releases.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn get_device(&self, Parameters(params): Parameters<DeviceParams>) -> ToolResult {
        let AxumJson(device) = call(device_route::get_device_info(
            Path(params.device),
            AxumExtension(self.state.clone()),
        ))
        .await?;
        json_result(&device)
    }

    /// List the distributions (OS images) releases are built for.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn list_distributions(&self) -> ToolResult {
        let AxumJson(distributions) = call(distribution_route::get_distributions(
            AxumExtension(self.state.clone()),
            Query(DistributionsFilter {
                include_archived: false,
            }),
        ))
        .await?;
        json_result(&distributions)
    }

    /// List releases, most recent first.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn list_releases(
        &self,
        Parameters(params): Parameters<ListReleasesParams>,
    ) -> ToolResult {
        let filter = ReleaseFilter { lts: params.lts };
        let AxumJson(mut releases) = match params.distribution_id {
            Some(distribution_id) => {
                call(distribution_route::get_distribution_releases(
                    Path(distribution_id),
                    AxumExtension(self.state.clone()),
                    Query(filter),
                ))
                .await?
            }
            None => {
                call(release_route::get_releases(
                    AxumExtension(self.state.clone()),
                    Query(filter),
                ))
                .await?
            }
        };
        releases.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        releases.truncate(params.limit.unwrap_or(20));
        json_result(&releases)
    }

    /// A release with its packages and deployment status, if it has been deployed.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn get_release(&self, Parameters(params): Parameters<ReleaseParams>) -> ToolResult {
        let release = self.release(params.release_id).await?;
        let AxumJson(packages) = call(release_route::get_distribution_release_packages(
            Path(params.release_id),
            AxumExtension(self.state.clone()),
        ))
        .await?;
        let deployment = match deployment_route::api_get_release_deployment(
            Path(params.release_id),
            AxumExtension(self.state.clone()),
        )
        .await
        {
            Ok((_, AxumJson(deployment))) => Some(deployment),
            Err(StatusCode::NOT_FOUND) => None,
            Err(status) => return Err(handler_error(status).await),
        };

        json_result(&json!({
            "release": release,
            "packages": packages,
            "deployment": deployment,
        }))
    }

    /// Create a new draft release by bumping the version of an existing release and copying its packages.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = false
    ))]
    async fn draft_release(
        &self,
        Parameters(params): Parameters<DraftReleaseParams>,
        Extension(parts): Extension<Parts>,
    ) -> ToolResult {
        let user = current_user(&parts)?;
        let AxumJson(releases) = call(distribution_route::get_distribution_releases(
            Path(params.distribution_id),
            AxumExtension(self.state.clone()),
            Query(ReleaseFilter::default()),
        ))
        .await?;

        // Releases come back newest first, matching `sm releases draft`.
        let base = match params.base_release_id {
            Some(id) => releases.iter().find(|release| release.id == id),
            None => releases.first(),
        }
        .ok_or_else(|| error_result("No base release found in this distribution to draft from."))?;

        let mut version = next_version(&base.version, params.bump.into()).ok_or_else(|| {
            error_result(format!(
                "Base release version {} is not semver (MAJOR.MINOR.PATCH).",
                base.version
            ))
        })?;
        if params.release_candidate {
            version = format!("{version}-rc");
        }

        let AxumJson(packages) = call(release_route::get_distribution_release_packages(
            Path(base.id),
            AxumExtension(self.state.clone()),
        ))
        .await?;

        let AxumJson(release_id) = call(distribution_route::create_distribution_release(
            AxumExtension(self.state.clone()),
            AxumExtension(user),
            Path(params.distribution_id),
            AxumJson(NewDistributionRelease {
                version,
                packages: packages.iter().map(|package| package.id).collect(),
                release_candidate: params.release_candidate,
            }),
        ))
        .await?;

        let release = self.release(release_id).await?;
        json_result(&json!({ "based_on": base.id, "release": release }))
    }

    /// Publish a draft release so it can be deployed.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn publish_release(&self, Parameters(params): Parameters<ReleaseParams>) -> ToolResult {
        self.update_release(
            params.release_id,
            UpdateRelease {
                draft: Some(false),
                yanked: None,
                lts: None,
            },
        )
        .await
    }

    /// Mark or unmark a release as LTS, the version new devices are flashed with.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn set_release_lts(&self, Parameters(params): Parameters<SetLtsParams>) -> ToolResult {
        self.update_release(
            params.release_id,
            UpdateRelease {
                draft: None,
                yanked: None,
                lts: Some(params.lts),
            },
        )
        .await
    }

    /// Yank (withdraw) a release so it is no longer offered to devices.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn yank_release(&self, Parameters(params): Parameters<ReleaseParams>) -> ToolResult {
        self.update_release(
            params.release_id,
            UpdateRelease {
                draft: None,
                yanked: Some(true),
                lts: None,
            },
        )
        .await
    }

    /// Promote a release candidate into a new regular release with the same packages and services.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = false
    ))]
    async fn promote_release_candidate(
        &self,
        Parameters(params): Parameters<PromoteParams>,
        Extension(parts): Extension<Parts>,
    ) -> ToolResult {
        let user = current_user(&parts)?;
        let (_, AxumJson(release_id)) = call(release_route::promote_release(
            Path(params.release_id),
            AxumExtension(self.state.clone()),
            AxumExtension(user),
            AxumJson(PromoteReleaseRequest {
                version: params.version,
            }),
        ))
        .await?;
        let release = self.release(release_id).await?;
        json_result(&release)
    }

    /// Start deploying a release to its canary devices. Devices update on their next check-in; follow up with get_deployment and confirm_full_rollout.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = false,
        open_world_hint = false
    ))]
    async fn deploy_release(
        &self,
        Parameters(params): Parameters<DeployParams>,
        Extension(parts): Extension<Parts>,
    ) -> ToolResult {
        let user = current_user(&parts)?;
        let request = (params.canary_device_labels.is_some() || params.canary_device_ids.is_some())
            .then_some(AxumJson(DeploymentRequest {
                canary_device_labels: params.canary_device_labels,
                canary_device_ids: params.canary_device_ids,
            }));
        let AxumJson(deployment) = call(deployment_route::api_release_deployment(
            Path(params.release_id),
            AxumExtension(self.state.clone()),
            AxumExtension(user),
            request,
        ))
        .await?;
        json_result(&deployment)
    }

    /// Deployment status of a release: which canary devices have updated and which services are unhealthy.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn get_deployment(&self, Parameters(params): Parameters<ReleaseParams>) -> ToolResult {
        let (_, AxumJson(deployment)) = call(deployment_route::api_get_release_deployment(
            Path(params.release_id),
            AxumExtension(self.state.clone()),
        ))
        .await?;
        let (_, AxumJson(devices)) = call(deployment_route::api_get_deployment_devices(
            Path(params.release_id),
            AxumExtension(self.state.clone()),
        ))
        .await?;
        let (_, AxumJson(health)) = call(deployment_route::api_get_deployment_service_health(
            Path(params.release_id),
            AxumExtension(self.state.clone()),
        ))
        .await?;

        let devices: Vec<_> = devices
            .iter()
            .map(|device| {
                json!({
                    "device_id": device.device_id,
                    "serial_number": device.serial_number,
                    "updated": device.release_id == Some(params.release_id),
                    "release_id": device.release_id,
                    "last_ping": device.last_ping,
                })
            })
            .collect();
        let unhealthy: Vec<_> = health
            .iter()
            .filter(|service| service.active_state != "active")
            .collect();

        json_result(&json!({
            "deployment": deployment,
            "devices": devices,
            "service_checks": health.len(),
            "unhealthy_services": unhealthy,
        }))
    }

    /// Roll a release out to the whole fleet once its canary devices are healthy.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = false,
        open_world_hint = false
    ))]
    async fn confirm_full_rollout(
        &self,
        Parameters(params): Parameters<ReleaseParams>,
        Extension(parts): Extension<Parts>,
    ) -> ToolResult {
        let user = current_user(&parts)?;
        let (_, AxumJson(deployment)) = call(deployment_route::api_confirm_full_rollout(
            Path(params.release_id),
            AxumExtension(self.state.clone()),
            AxumExtension(user),
        ))
        .await?;
        json_result(&deployment)
    }

    /// Fetch journal logs from a device (journalctl). Waits briefly for the device to answer.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn get_device_logs(
        &self,
        Parameters(params): Parameters<LogsParams>,
        Extension(parts): Extension<Parts>,
    ) -> ToolResult {
        let devices = self.resolve_devices(vec![params.device]).await?;
        let receipt = self
            .queue(
                &parts,
                devices,
                vec![SafeCommandTx::GetLogs {
                    unit: params.unit,
                    since: params.since,
                    until: params.until,
                    grep: params.grep,
                }],
            )
            .await?;
        self.bundle_result(
            receipt.uuid,
            params.wait_seconds.unwrap_or(DEFAULT_WAIT_SECONDS),
        )
        .await
    }

    /// Run `systemctl status` for a unit on devices. Requires the freeform command permission.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn get_service_status(
        &self,
        Parameters(params): Parameters<ServiceParams>,
        Extension(parts): Extension<Parts>,
    ) -> ToolResult {
        self.systemctl(&parts, "status", params).await
    }

    /// Run `systemctl restart` for a unit on devices. Requires the freeform command permission.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = false,
        open_world_hint = false
    ))]
    async fn restart_service(
        &self,
        Parameters(params): Parameters<ServiceParams>,
        Extension(parts): Extension<Parts>,
    ) -> ToolResult {
        self.systemctl(&parts, "restart", params).await
    }

    /// Reboot devices. Returns a bundle UUID for get_command_results.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = false,
        open_world_hint = false
    ))]
    async fn restart_devices(
        &self,
        Parameters(params): Parameters<DevicesParams>,
        Extension(parts): Extension<Parts>,
    ) -> ToolResult {
        let devices = self.resolve_devices(params.devices).await?;
        let receipt = self
            .queue(&parts, devices, vec![SafeCommandTx::Restart])
            .await?;
        json_result(&receipt)
    }

    /// Results of queued device commands, by bundle UUID.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn get_command_results(
        &self,
        Parameters(params): Parameters<CommandResultsParams>,
    ) -> ToolResult {
        let uuid = Uuid::parse_str(&params.bundle_uuid)
            .map_err(|err| error_result(format!("Invalid bundle UUID: {err}")))?;
        self.bundle_result(uuid, params.wait_seconds.unwrap_or(0))
            .await
    }

    /// List saved command recipes: vetted command bundles that can be run against devices.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn list_recipes(&self) -> ToolResult {
        let AxumJson(recipes) = call(command_route::get_recipes(AxumExtension(
            self.state.clone(),
        )))
        .await?;
        json_result(&recipes)
    }

    /// Run a saved recipe against devices. Returns a bundle UUID for get_command_results.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = false,
        open_world_hint = false
    ))]
    async fn trigger_recipe(
        &self,
        Parameters(params): Parameters<TriggerRecipeParams>,
        Extension(parts): Extension<Parts>,
    ) -> ToolResult {
        let user = current_user(&parts)?;
        let devices = self.resolve_devices(params.devices).await?;
        let (_, AxumJson(receipt)) = call(command_route::trigger_recipe(
            AxumExtension(self.state.clone()),
            AxumExtension(user),
            Path(params.recipe_id),
            AxumJson(TriggerRecipeInput { devices }),
        ))
        .await?;
        json_result(&receipt)
    }
}

impl SmithMcp {
    async fn release(&self, release_id: i32) -> Result<Release, CallToolResult> {
        let AxumJson(release) = call(release_route::get_release(
            Path(release_id),
            AxumExtension(self.state.clone()),
        ))
        .await?;
        Ok(release)
    }

    async fn update_release(&self, release_id: i32, update: UpdateRelease) -> ToolResult {
        call(release_route::update_release(
            Path(release_id),
            AxumExtension(self.state.clone()),
            AxumJson(update),
        ))
        .await?;
        let release = self.release(release_id).await?;
        json_result(&release)
    }

    async fn resolve_devices(&self, devices: Vec<String>) -> Result<Vec<i32>, CallToolResult> {
        if devices.is_empty() {
            return Err(error_result(
                "Provide at least one device id or serial number.",
            ));
        }
        if devices.len() > MAX_TARGET_DEVICES {
            return Err(error_result(format!(
                "At most {MAX_TARGET_DEVICES} devices can be targeted at once; use a deployment or narrow the selection."
            )));
        }

        let mut ids = Vec::with_capacity(devices.len());
        for device in devices {
            let AxumJson(info) = call(device_route::get_device_info(
                Path(device.clone()),
                AxumExtension(self.state.clone()),
            ))
            .await
            .map_err(|_| error_result(format!("Device {device} was not found.")))?;
            ids.push(info.id);
        }
        Ok(ids)
    }

    async fn queue(
        &self,
        parts: &Parts,
        devices: Vec<i32>,
        commands: Vec<SafeCommandTx>,
    ) -> Result<BundleReceipt, CallToolResult> {
        let user = current_user(parts)?;
        let commands = commands
            .into_iter()
            .map(|command| SafeCommandRequest {
                id: 0,
                command,
                continue_on_error: false,
            })
            .collect();
        let (_, AxumJson(receipt)) = call(command_route::issue_commands_to_devices(
            AxumExtension(self.state.clone()),
            AxumExtension(user),
            AxumJson(BundleCommands { devices, commands }),
        ))
        .await?;
        Ok(receipt)
    }

    async fn systemctl(&self, parts: &Parts, action: &str, params: ServiceParams) -> ToolResult {
        // The unit is interpolated into a shell command on the device.
        let valid_unit = !params.unit.is_empty()
            && params.unit.len() <= 256
            && !params.unit.starts_with('-')
            && params
                .unit
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "@._:-".contains(c));
        if !valid_unit {
            return Err(error_result(format!(
                "Invalid systemd unit name: {}",
                params.unit
            )));
        }

        let devices = self.resolve_devices(params.devices).await?;
        let receipt = self
            .queue(
                parts,
                devices,
                vec![SafeCommandTx::FreeForm {
                    cmd: format!("systemctl {action} {}", params.unit),
                }],
            )
            .await?;
        self.bundle_result(
            receipt.uuid,
            params.wait_seconds.unwrap_or(DEFAULT_WAIT_SECONDS),
        )
        .await
    }

    async fn bundle_result(&self, uuid: Uuid, wait_seconds: u64) -> ToolResult {
        let deadline = Instant::now() + Duration::from_secs(wait_seconds.min(MAX_WAIT_SECONDS));
        loop {
            let bundle = self.bundle(uuid).await?;
            let complete = bundle
                .responses
                .iter()
                .all(|command| command.response_id.is_some() || command.cancelled);
            if complete || Instant::now() >= deadline {
                return json_result(&json!({
                    "bundle_uuid": uuid,
                    "complete": complete,
                    "commands": bundle.responses,
                }));
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    async fn bundle(&self, uuid: Uuid) -> Result<BundleWithCommands, CallToolResult> {
        let AxumJson(bundle) = call(command_route::get_bundle(
            AxumExtension(self.state.clone()),
            Path(uuid),
        ))
        .await?;
        Ok(bundle)
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for SmithMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(INSTRUCTIONS)
    }
}

fn release_summary(release: &Release) -> serde_json::Value {
    json!({
        "id": release.id,
        "version": release.version,
        "distribution": release.distribution_name,
    })
}

fn current_user(parts: &Parts) -> Result<CurrentUser, CallToolResult> {
    parts
        .extensions
        .get::<CurrentUser>()
        .cloned()
        .ok_or_else(|| {
            error!("MCP tool called without an authenticated user");
            error_result("Not authenticated.")
        })
}

fn json_result<T: Serialize>(value: &T) -> ToolResult {
    serde_json::to_string_pretty(value)
        .map(|text| CallToolResult::success(vec![ContentBlock::text(text)]))
        .map_err(|err| error_result(format!("Failed to serialize result: {err}")))
}

fn error_result(message: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message.into())])
}

async fn call<T, E: IntoResponse>(
    handler: impl Future<Output = Result<T, E>>,
) -> Result<T, CallToolResult> {
    match handler.await {
        Ok(value) => Ok(value),
        Err(err) => Err(handler_error(err).await),
    }
}

/// Handlers report failures as HTTP responses; turn one into a tool error the
/// model can read and act on.
async fn handler_error(err: impl IntoResponse) -> CallToolResult {
    let response = err.into_response();
    let status = response.status();
    let body = match axum::body::to_bytes(response.into_body(), 64 * 1024).await {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(err) => {
            error!("Failed to read handler error body: {err}");
            String::new()
        }
    };
    let hint = match status {
        StatusCode::FORBIDDEN => " Your Smith role does not allow this action.",
        StatusCode::NOT_FOUND => " The requested resource does not exist.",
        _ => "",
    };
    error_result(
        format!("Request failed with {status}.{hint} {body}")
            .trim_end()
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::SmithMcp;

    #[test]
    fn tools_declare_whether_they_change_the_fleet() {
        let tools = SmithMcp::tool_router().list_all();
        assert_eq!(tools.len(), 20);

        // Clients decide whether to ask for confirmation from these hints, so a
        // mutating tool must never be left unannotated.
        for tool in &tools {
            let annotations = tool
                .annotations
                .as_ref()
                .unwrap_or_else(|| panic!("{} has no annotations", tool.name));
            assert!(
                annotations.read_only_hint == Some(true) || annotations.destructive_hint.is_some(),
                "{} must declare whether it is destructive",
                tool.name
            );
        }

        for name in [
            "deploy_release",
            "confirm_full_rollout",
            "yank_release",
            "restart_devices",
            "restart_service",
            "trigger_recipe",
        ] {
            let tool = tools
                .iter()
                .find(|tool| tool.name == name)
                .unwrap_or_else(|| panic!("{name} is not registered"));
            assert_eq!(
                tool.annotations
                    .as_ref()
                    .and_then(|annotations| annotations.destructive_hint),
                Some(true),
                "{name} must be marked destructive"
            );
        }
    }
}
