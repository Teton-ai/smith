//! Device settings: what devices report, the pins and distribution defaults
//! they should run, and the edit and Apply endpoints that send them.

use super::route::{DEVICES_TAG, resolve_device_id};
use crate::State;
use crate::command::route::queue_bundle;
use crate::middlewares::authorization;
use crate::user::CurrentUser;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::{Extension, Json};
use models::command::BundleReceipt;
use serde::Deserialize;
use smith::utils::schema;
use smith::utils::schema::SafeCommandRequest;
use sqlx::types::chrono::{DateTime, Utc};
use tracing::error;

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct ReportedSettings {
    #[serde(flatten)]
    pub report: schema::SettingsReport,
    /// When the device last sent this report (on change, daily or on request).
    pub reported_at: DateTime<Utc>,
}

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct DeviceSettings {
    /// `None` when the device never reported (smithd without settings
    /// reporting).
    pub reported: Option<ReportedSettings>,
    /// Settings devices of this distribution cannot have; show them as not
    /// used rather than as read errors.
    pub unsupported: Vec<schema::DeviceSetting>,
    /// The device's pins; `None` fields inherit the distribution default.
    pub wanted: SettingValues,
    /// The distribution defaults; `None` fields are not managed.
    pub defaults: SettingValues,
    /// What an Apply would send now.
    pub resolved: schema::WantedSettings,
    /// Bumped by every edit that changes the pins; ahead of the reported
    /// `applied_version` while edits are not applied.
    pub settings_version: i32,
}

/// One value per managed field, as stored in `device_settings` and
/// `distribution_setting_default`.
#[derive(serde::Serialize, Deserialize, Default, Debug, Clone, PartialEq, utoipa::ToSchema)]
#[serde(default)]
pub struct SettingValues {
    pub wireless_mode: Option<i16>,
    pub power_mgnt: Option<i16>,
    pub roam_flags: Option<i16>,
    pub idle_diff: Option<i32>,
    pub busy_diff: Option<i32>,
    pub scanr_exp_ms: Option<i32>,
    pub scan_int: Option<i32>,
    pub idle_th: Option<i32>,
    pub busy_th: Option<i32>,
    pub nm_wifi_powersave: Option<i16>,
    pub nm_autoconnect_retries: Option<i32>,
    pub nm_cloned_mac: Option<String>,
    pub nm_scan_rand_mac: Option<bool>,
    pub hostname: Option<String>,
    pub ntp_servers: Option<Vec<String>>,
    pub bgscan: Option<String>,
}

impl SettingValues {
    fn roam_tuning(&self) -> Option<schema::RoamTuning> {
        Some(schema::RoamTuning {
            idle_diff: self.idle_diff?,
            busy_diff: self.busy_diff,
            scanr_exp_ms: self.scanr_exp_ms?,
            scan_int: self.scan_int?,
            idle_th: self.idle_th?,
            busy_th: self.busy_th,
        })
    }

    fn mac(&self) -> Option<schema::MacRandomization> {
        Some(schema::MacRandomization {
            cloned_mac: self.nm_cloned_mac.clone()?,
            scan_rand_mac: self.nm_scan_rand_mac?,
        })
    }
}

/// Pin, else distribution default, else not managed. A setting the
/// distribution does not support is left out, so the device leaves it alone.
/// Groups resolve whole: a pinned roam tuning never mixes with the default's
/// busy fields.
pub fn resolve_settings(
    pins: &SettingValues,
    defaults: &SettingValues,
    unsupported: &[schema::DeviceSetting],
) -> schema::WantedSettings {
    use schema::{DeviceSetting, Managed};

    fn group<T>(supported: bool, pin: Option<T>, default: Option<T>) -> Option<Managed<T>> {
        supported.then(|| pin.or(default).map_or(Managed::Unmanaged, Managed::Set))
    }
    let supported = |setting: DeviceSetting| !unsupported.contains(&setting);

    let roam_tuning = group(
        supported(DeviceSetting::IdleDiff),
        pins.roam_tuning(),
        defaults.roam_tuning(),
    )
    .map(|tuning| match tuning {
        Managed::Set(mut tuning) => {
            if !supported(DeviceSetting::BusyDiff) {
                tuning.busy_diff = None;
            }
            if !supported(DeviceSetting::BusyTh) {
                tuning.busy_th = None;
            }
            Managed::Set(tuning)
        }
        Managed::Unmanaged => Managed::Unmanaged,
    });

    schema::WantedSettings {
        wireless_mode: group(
            supported(DeviceSetting::WirelessMode),
            pins.wireless_mode,
            defaults.wireless_mode,
        ),
        power_mgnt: group(
            supported(DeviceSetting::PowerMgnt),
            pins.power_mgnt,
            defaults.power_mgnt,
        ),
        roam_flags: group(
            supported(DeviceSetting::RoamFlags),
            pins.roam_flags,
            defaults.roam_flags,
        ),
        roam_tuning,
        nm_wifi_powersave: group(
            supported(DeviceSetting::NmWifiPowersave),
            pins.nm_wifi_powersave,
            defaults.nm_wifi_powersave,
        ),
        nm_autoconnect_retries: group(
            supported(DeviceSetting::NmAutoconnectRetries),
            pins.nm_autoconnect_retries,
            defaults.nm_autoconnect_retries,
        ),
        mac: group(
            supported(DeviceSetting::NmClonedMac) && supported(DeviceSetting::NmScanRandMac),
            pins.mac(),
            defaults.mac(),
        ),
        hostname: group(
            supported(DeviceSetting::Hostname),
            pins.hostname.clone(),
            defaults.hostname.clone(),
        ),
        ntp_servers: group(
            supported(DeviceSetting::NtpServers),
            pins.ntp_servers.clone(),
            defaults.ntp_servers.clone(),
        ),
        bgscan: group(
            supported(DeviceSetting::Bgscan),
            pins.bgscan.clone(),
            defaults.bgscan.clone(),
        ),
    }
}

pub struct SettingsInputs {
    pub device_id: i32,
    pub settings_version: i32,
    pub pins: SettingValues,
    pub defaults: SettingValues,
    pub unsupported: Vec<schema::DeviceSetting>,
}

impl SettingsInputs {
    pub fn resolve(&self) -> schema::WantedSettings {
        resolve_settings(&self.pins, &self.defaults, &self.unsupported)
    }
}

/// Linux `HOST_NAME_MAX`.
const HOST_NAME_MAX: usize = 64;
/// Longest DNS name.
const DNS_NAME_MAX: usize = 253;
const BGSCAN_MAX: usize = 128;

/// Dot separated labels of letters, digits and inner hyphens.
fn is_dns_name(name: &str, max_len: usize) -> bool {
    !name.is_empty()
        && name.len() <= max_len
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// A host name or an IP address, as chrony and timesyncd take it.
fn is_ntp_server(server: &str) -> bool {
    server.parse::<std::net::IpAddr>().is_ok() || is_dns_name(server, DNS_NAME_MAX)
}

/// NetworkManager's `cloned-mac-address` keywords, or a MAC address.
fn is_cloned_mac(value: &str) -> bool {
    matches!(value, "preserve" | "permanent" | "random" | "stable")
        || (value.len() == 17
            && value
                .split(':')
                .all(|octet| octet.len() == 2 && octet.bytes().all(|b| b.is_ascii_hexdigit())))
}

/// wpa_supplicant's `module:parameters` (e.g. `simple:30:-65:300`); empty
/// turns background scanning off.
fn is_bgscan(value: &str) -> bool {
    value.len() <= BGSCAN_MAX
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b":-_./".contains(&b))
}

/// `{"pin": value}` pins the group; `"default"` removes the pin so the group
/// inherits the distribution default again.
#[derive(serde::Serialize, Deserialize, Debug, Clone, PartialEq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SettingChange<T> {
    Pin(T),
    Default,
}

/// Groups to change; a group left out keeps its current pin or default.
#[derive(serde::Serialize, Deserialize, Default, Debug, Clone, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SettingsChanges {
    pub wireless_mode: Option<SettingChange<i16>>,
    pub power_mgnt: Option<SettingChange<i16>>,
    pub roam_flags: Option<SettingChange<i16>>,
    pub roam_tuning: Option<SettingChange<schema::RoamTuning>>,
    pub nm_wifi_powersave: Option<SettingChange<i16>>,
    pub nm_autoconnect_retries: Option<SettingChange<i32>>,
    pub mac: Option<SettingChange<schema::MacRandomization>>,
    pub hostname: Option<SettingChange<String>>,
    pub ntp_servers: Option<SettingChange<Vec<String>>>,
    pub bgscan: Option<SettingChange<String>>,
}

impl SettingsChanges {
    /// Settings these changes pin, to check against the distribution.
    fn pinned(&self) -> Vec<schema::DeviceSetting> {
        use schema::DeviceSetting as S;
        fn is_pin<T>(change: &Option<SettingChange<T>>) -> bool {
            matches!(change, Some(SettingChange::Pin(_)))
        }
        let mut pinned = Vec::new();
        for (change, setting) in [
            (is_pin(&self.wireless_mode), S::WirelessMode),
            (is_pin(&self.power_mgnt), S::PowerMgnt),
            (is_pin(&self.roam_flags), S::RoamFlags),
            (is_pin(&self.nm_wifi_powersave), S::NmWifiPowersave),
            (
                is_pin(&self.nm_autoconnect_retries),
                S::NmAutoconnectRetries,
            ),
            (is_pin(&self.hostname), S::Hostname),
            (is_pin(&self.ntp_servers), S::NtpServers),
            (is_pin(&self.bgscan), S::Bgscan),
        ] {
            if change {
                pinned.push(setting);
            }
        }
        if let Some(SettingChange::Pin(tuning)) = &self.roam_tuning {
            pinned.extend([S::IdleDiff, S::ScanrExpMs, S::ScanInt, S::IdleTh]);
            if tuning.busy_diff.is_some() {
                pinned.push(S::BusyDiff);
            }
            if tuning.busy_th.is_some() {
                pinned.push(S::BusyTh);
            }
        }
        if is_pin(&self.mac) {
            pinned.extend([S::NmClonedMac, S::NmScanRandMac]);
        }
        pinned
    }

    /// smithd writes these values into root owned files (`/etc/hosts`, the
    /// chrony and NM drop-ins), so anything that could add a line or an
    /// option is refused here. Going back to the default is `"default"`.
    fn is_valid(&self) -> bool {
        fn pinned<T>(change: &Option<SettingChange<T>>, valid: impl Fn(&T) -> bool) -> bool {
            match change {
                Some(SettingChange::Pin(value)) => valid(value),
                _ => true,
            }
        }
        pinned(&self.hostname, |name| is_dns_name(name, HOST_NAME_MAX))
            && pinned(&self.ntp_servers, |servers| {
                !servers.is_empty() && servers.iter().all(|server| is_ntp_server(server))
            })
            && pinned(&self.mac, |mac| is_cloned_mac(&mac.cloned_mac))
            && pinned(&self.bgscan, |bgscan| is_bgscan(bgscan))
    }

    fn apply_to(&self, pins: &SettingValues) -> SettingValues {
        fn change<T: Clone>(slot: &mut Option<T>, change: &Option<SettingChange<T>>) {
            match change {
                Some(SettingChange::Pin(value)) => *slot = Some(value.clone()),
                Some(SettingChange::Default) => *slot = None,
                None => {}
            }
        }
        let mut new = pins.clone();
        change(&mut new.wireless_mode, &self.wireless_mode);
        change(&mut new.power_mgnt, &self.power_mgnt);
        change(&mut new.roam_flags, &self.roam_flags);
        change(&mut new.nm_wifi_powersave, &self.nm_wifi_powersave);
        change(
            &mut new.nm_autoconnect_retries,
            &self.nm_autoconnect_retries,
        );
        change(&mut new.hostname, &self.hostname);
        change(&mut new.ntp_servers, &self.ntp_servers);
        change(&mut new.bgscan, &self.bgscan);
        match &self.roam_tuning {
            Some(SettingChange::Pin(tuning)) => {
                new.idle_diff = Some(tuning.idle_diff);
                new.busy_diff = tuning.busy_diff;
                new.scanr_exp_ms = Some(tuning.scanr_exp_ms);
                new.scan_int = Some(tuning.scan_int);
                new.idle_th = Some(tuning.idle_th);
                new.busy_th = tuning.busy_th;
            }
            Some(SettingChange::Default) => {
                new.idle_diff = None;
                new.busy_diff = None;
                new.scanr_exp_ms = None;
                new.scan_int = None;
                new.idle_th = None;
                new.busy_th = None;
            }
            None => {}
        }
        match &self.mac {
            Some(SettingChange::Pin(mac)) => {
                new.nm_cloned_mac = Some(mac.cloned_mac.clone());
                new.nm_scan_rand_mac = Some(mac.scan_rand_mac);
            }
            Some(SettingChange::Default) => {
                new.nm_cloned_mac = None;
                new.nm_scan_rand_mac = None;
            }
            None => {}
        }
        new
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DevicesSettingsChanges {
    pub devices: Vec<i32>,
    pub changes: SettingsChanges,
}

#[derive(serde::Serialize, Debug, PartialEq, utoipa::ToSchema)]
pub struct DeviceSettingsVersion {
    pub device_id: i32,
    /// Unchanged when the edit left the device's pins as they were.
    pub settings_version: i32,
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsEditError {
    #[error("device not found")]
    NotFound,
    #[error("no devices, an invalid value, or a hostname pinned on several devices")]
    Invalid,
    #[error("a pinned setting is not supported on the device's distribution")]
    Unsupported,
    #[error("failed to encode the settings: {0}")]
    Encode(#[from] serde_json::Error),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

impl From<SettingsEditError> for StatusCode {
    fn from(err: SettingsEditError) -> Self {
        match err {
            SettingsEditError::NotFound => StatusCode::NOT_FOUND,
            SettingsEditError::Invalid => StatusCode::BAD_REQUEST,
            SettingsEditError::Unsupported => StatusCode::UNPROCESSABLE_ENTITY,
            SettingsEditError::Encode(err) => {
                error!("Failed to encode settings changes: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            }
            SettingsEditError::Db(err) => {
                error!("Failed to save settings changes: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }
}

/// Writes the changed pins of every device and bumps the settings version of
/// those whose pins changed. Queues nothing: an Apply sends them. All or
/// nothing: one unknown or archived device, or one unsupported pin, rejects
/// the whole edit. A hostname is pinned one device at a time: the same name on
/// several devices is never what was meant.
pub async fn save_settings_changes(
    pool: &sqlx::PgPool,
    device_ids: &[i32],
    changes: &SettingsChanges,
    user_id: i32,
) -> Result<Vec<DeviceSettingsVersion>, SettingsEditError> {
    if device_ids.is_empty() || !changes.is_valid() {
        return Err(SettingsEditError::Invalid);
    }
    let mut tx = pool.begin().await?;
    // Read by the audit trigger.
    sqlx::query("SELECT set_config('smith.user_id', $1, true)")
        .bind(user_id.to_string())
        .execute(&mut *tx)
        .await?;
    let mut requested = device_ids.to_vec();
    requested.sort_unstable();
    requested.dedup();
    if requested.len() > 1 && matches!(changes.hostname, Some(SettingChange::Pin(_))) {
        return Err(SettingsEditError::Invalid);
    }
    // Serializes concurrent edits of the same devices.
    let locked = sqlx::query_scalar!(
        "SELECT id FROM device WHERE id = ANY($1) AND NOT archived ORDER BY id FOR UPDATE",
        &requested
    )
    .fetch_all(&mut *tx)
    .await?;
    if locked != requested {
        return Err(SettingsEditError::NotFound);
    }

    let inputs = load_settings_inputs(&mut *tx, &requested).await?;
    let pinned = changes.pinned();
    if inputs
        .iter()
        .any(|input| pinned.iter().any(|s| input.unsupported.contains(s)))
    {
        return Err(SettingsEditError::Unsupported);
    }

    let mut versions = Vec::with_capacity(inputs.len());
    let mut changed = Vec::new();
    for input in &inputs {
        let new = changes.apply_to(&input.pins);
        if new == input.pins {
            versions.push(DeviceSettingsVersion {
                device_id: input.device_id,
                settings_version: input.settings_version,
            });
            continue;
        }
        if new == SettingValues::default() {
            sqlx::query!(
                "DELETE FROM device_settings WHERE device_id = $1",
                input.device_id
            )
            .execute(&mut *tx)
            .await?;
        } else {
            let values = serde_json::to_value(&new)?;
            sqlx::query!(
                r#"
                INSERT INTO device_settings
                SELECT * FROM jsonb_populate_record(
                    NULL::device_settings,
                    $2::jsonb || jsonb_build_object(
                        'device_id', $1::int4, 'updated_at', now(), 'updated_by', $3::int4
                    )
                )
                ON CONFLICT (device_id) DO UPDATE SET
                    updated_at = EXCLUDED.updated_at,
                    updated_by = EXCLUDED.updated_by,
                    wireless_mode = EXCLUDED.wireless_mode,
                    power_mgnt = EXCLUDED.power_mgnt,
                    roam_flags = EXCLUDED.roam_flags,
                    idle_diff = EXCLUDED.idle_diff,
                    busy_diff = EXCLUDED.busy_diff,
                    scanr_exp_ms = EXCLUDED.scanr_exp_ms,
                    scan_int = EXCLUDED.scan_int,
                    idle_th = EXCLUDED.idle_th,
                    busy_th = EXCLUDED.busy_th,
                    nm_wifi_powersave = EXCLUDED.nm_wifi_powersave,
                    nm_autoconnect_retries = EXCLUDED.nm_autoconnect_retries,
                    nm_cloned_mac = EXCLUDED.nm_cloned_mac,
                    nm_scan_rand_mac = EXCLUDED.nm_scan_rand_mac,
                    hostname = EXCLUDED.hostname,
                    ntp_servers = EXCLUDED.ntp_servers,
                    bgscan = EXCLUDED.bgscan
                "#,
                input.device_id,
                values,
                user_id,
            )
            .execute(&mut *tx)
            .await?;
        }
        changed.push(input.device_id);
    }

    let bumped = sqlx::query!(
        "UPDATE device SET settings_version = settings_version + 1
         WHERE id = ANY($1)
         RETURNING id, settings_version",
        &changed
    )
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    versions.extend(bumped.into_iter().map(|row| DeviceSettingsVersion {
        device_id: row.id,
        settings_version: row.settings_version,
    }));
    versions.sort_by_key(|v| v.device_id);
    Ok(versions)
}

#[utoipa::path(
    put,
    path = "/devices/{device_id}/settings",
    params(
        ("device_id" = String, Path, description = "Device ID or serial number"),
    ),
    request_body = SettingsChanges,
    responses(
        (status = StatusCode::OK, description = "Pins saved; nothing is sent until an Apply", body = DeviceSettingsVersion),
        (status = StatusCode::BAD_REQUEST, description = "An invalid hostname, NTP server, cloned MAC or bgscan value"),
        (status = StatusCode::FORBIDDEN, description = "Missing devices:write"),
        (status = StatusCode::NOT_FOUND, description = "Device not found or archived"),
        (status = StatusCode::UNPROCESSABLE_ENTITY, description = "A pinned setting is not supported on the device's distribution"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to save the settings"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn update_settings_for_device(
    Path(device_id): Path<String>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Json(changes): Json<SettingsChanges>,
) -> Result<Json<DeviceSettingsVersion>, StatusCode> {
    let user_id = current_user.user_id;
    if !authorization::check(current_user, "devices", "write") {
        return Err(StatusCode::FORBIDDEN);
    }
    let resolved_id = resolve_device_id(&device_id, &state.pg_pool).await?;
    save_settings_changes(&state.pg_pool, &[resolved_id], &changes, user_id)
        .await?
        .pop()
        .map(Json)
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)
}

#[utoipa::path(
    put,
    path = "/devices/settings",
    request_body = DevicesSettingsChanges,
    responses(
        (status = StatusCode::OK, description = "Pins saved on every device; nothing is sent until an Apply", body = Vec<DeviceSettingsVersion>),
        (status = StatusCode::BAD_REQUEST, description = "No devices, an invalid value, or a hostname pinned on several devices"),
        (status = StatusCode::FORBIDDEN, description = "Missing devices:write"),
        (status = StatusCode::NOT_FOUND, description = "A device was not found or is archived"),
        (status = StatusCode::UNPROCESSABLE_ENTITY, description = "A pinned setting is not supported on a device's distribution"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to save the settings"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn update_settings_for_devices(
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Json(body): Json<DevicesSettingsChanges>,
) -> Result<Json<Vec<DeviceSettingsVersion>>, StatusCode> {
    let user_id = current_user.user_id;
    if !authorization::check(current_user, "devices", "write") {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(Json(
        save_settings_changes(&state.pg_pool, &body.devices, &body.changes, user_id).await?,
    ))
}

/// The devices of a bulk Apply, as selected (and filtered) on the Devices page.
#[derive(Deserialize, Debug, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplySettingsDevices {
    pub devices: Vec<i32>,
}

fn can_apply_settings(current_user: CurrentUser) -> bool {
    let command = authorization::required_permission(&schema::SafeCommandTx::ApplySettings {
        version: 0,
        settings: Default::default(),
    });
    current_user.has_permission(&command.resource, &command.action)
        && authorization::check(current_user, "devices", "write")
}

/// Who queues an Apply: a user, or the API for a device nobody has edited yet
/// (settings version 0).
enum ApplyBy {
    User(i32),
    AutoApply,
}

/// One `ApplySettings` per device with its resolved set at its current
/// version, queued as one staggered bundle. Devices at version 0 have nothing
/// to apply and are skipped; `None` when none is left.
async fn queue_settings_apply(
    pool: &sqlx::PgPool,
    device_ids: &[i32],
    by: ApplyBy,
) -> Result<Option<BundleReceipt>, StatusCode> {
    let db_error = |context: &'static str| {
        move |err: sqlx::Error| {
            error!("{context}: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    let mut tx = pool
        .begin()
        .await
        .map_err(db_error("Failed to start transaction"))?;
    // No row lock: every home poll writes its device row, so locking thousands
    // of devices would stall their polls until the bundle commits. An edit
    // landing after the resolution only leaves the device pending.
    let (device_ids, user_id) = match by {
        ApplyBy::User(user_id) => (device_ids.to_vec(), Some(user_id)),
        // Version 0 is what Apply skips, so the claim moves it to 1 in this
        // transaction; a second poll that also saw 0 finds 1 and queues nothing.
        ApplyBy::AutoApply => {
            let claimed = sqlx::query_scalar!(
                "UPDATE device SET settings_version = 1
                 WHERE id = ANY($1) AND settings_version = 0
                 RETURNING id",
                device_ids
            )
            .fetch_all(&mut *tx)
            .await
            .map_err(db_error("Failed to claim the automatic settings apply"))?;
            (claimed, None)
        }
    };
    if device_ids.is_empty() {
        return Ok(None);
    }
    let commands: Vec<(i32, SafeCommandRequest)> = load_settings_inputs(&mut *tx, &device_ids)
        .await
        .map_err(db_error("Failed to resolve settings to apply"))?
        .into_iter()
        .filter(|input| input.settings_version > 0)
        .map(|input| {
            (
                input.device_id,
                SafeCommandRequest {
                    id: 0,
                    command: schema::SafeCommandTx::ApplySettings {
                        version: input.settings_version,
                        settings: input.resolve(),
                    },
                    continue_on_error: false,
                },
            )
        })
        .collect();
    if commands.is_empty() {
        return Ok(None);
    }
    let entries: Vec<(i32, &[SafeCommandRequest])> = commands
        .iter()
        .map(|(device, command)| (*device, std::slice::from_ref(command)))
        .collect();
    let receipt = queue_bundle(&mut tx, &entries, user_id).await?;
    tx.commit()
        .await
        .map_err(db_error("Failed to commit the settings apply"))?;
    Ok(Some(receipt))
}

#[utoipa::path(
    post,
    path = "/devices/{device_id}/settings/apply",
    params(
        ("device_id" = String, Path, description = "Device ID or serial number"),
    ),
    responses(
        (status = StatusCode::ACCEPTED, description = "ApplySettings queued with the resolved set; also re-applies an applied version", body = BundleReceipt),
        (status = StatusCode::BAD_REQUEST, description = "Nothing to apply (settings version 0)"),
        (status = StatusCode::FORBIDDEN, description = "Missing devices:write or commands:basic"),
        (status = StatusCode::NOT_FOUND, description = "Device not found or archived"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to queue the command"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn apply_settings_for_device(
    Path(device_id): Path<String>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
) -> Result<(StatusCode, Json<BundleReceipt>), StatusCode> {
    let user_id = current_user.user_id;
    if !can_apply_settings(current_user) {
        return Err(StatusCode::FORBIDDEN);
    }
    let resolved_id = resolve_device_id(&device_id, &state.pg_pool).await?;
    let device_ids = active_devices(&state.pg_pool, &[resolved_id]).await?;
    queue_settings_apply(&state.pg_pool, &device_ids, ApplyBy::User(user_id))
        .await?
        .map(|receipt| (StatusCode::ACCEPTED, Json(receipt)))
        .ok_or(StatusCode::BAD_REQUEST)
}

#[utoipa::path(
    post,
    path = "/devices/settings/apply",
    request_body = ApplySettingsDevices,
    responses(
        (status = StatusCode::ACCEPTED, description = "ApplySettings queued for every listed device with something to apply, as one staggered bundle", body = BundleReceipt),
        (status = StatusCode::BAD_REQUEST, description = "No devices, or nothing to apply"),
        (status = StatusCode::FORBIDDEN, description = "Missing devices:write or commands:basic"),
        (status = StatusCode::NOT_FOUND, description = "A device id was not found or is archived"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to queue the commands"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn apply_settings_for_devices(
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Json(body): Json<ApplySettingsDevices>,
) -> Result<(StatusCode, Json<BundleReceipt>), StatusCode> {
    let user_id = current_user.user_id;
    if !can_apply_settings(current_user) {
        return Err(StatusCode::FORBIDDEN);
    }
    let device_ids = active_devices(&state.pg_pool, &body.devices).await?;
    queue_settings_apply(&state.pg_pool, &device_ids, ApplyBy::User(user_id))
        .await?
        .map(|receipt| (StatusCode::ACCEPTED, Json(receipt)))
        .ok_or(StatusCode::BAD_REQUEST)
}

/// Called on every home poll. A device nobody has edited yet (settings version
/// 0) gets its resolved set queued without an Apply press, once it has a pin or
/// its distribution has defaults, so a new device picks up its defaults by
/// itself. Existing devices are moved to version 1 by the launch migration in
/// the same transaction that adds the first defaults, so they are never picked
/// up here.
pub async fn auto_apply_settings(pool: &sqlx::PgPool, device_id: i32) -> Result<(), StatusCode> {
    // One indexed read per poll; the resolution only runs for a device that is
    // due.
    let due = sqlx::query_scalar!(
        r#"
        SELECT d.settings_version = 0 AND NOT d.archived AND (
            EXISTS (SELECT 1 FROM device_settings ds WHERE ds.device_id = d.id)
            OR EXISTS (
                SELECT 1 FROM distribution_setting_default dsd
                WHERE dsd.distribution_id = r.distribution_id
                  AND jsonb_strip_nulls(to_jsonb(dsd) - 'distribution_id') <> '{}'::jsonb
            )
        ) AS "due!"
        FROM device d
        LEFT JOIN release r ON r.id = d.release_id
        WHERE d.id = $1
        "#,
        device_id
    )
    .fetch_optional(pool)
    .await
    .map_err(|err| {
        error!(
            device_id,
            "Failed to check the automatic settings apply: {err}"
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .unwrap_or(false);
    if !due {
        return Ok(());
    }
    queue_settings_apply(pool, &[device_id], ApplyBy::AutoApply).await?;
    Ok(())
}

/// `devices` sorted and deduplicated; 404 when one is unknown or archived.
async fn active_devices(pool: &sqlx::PgPool, devices: &[i32]) -> Result<Vec<i32>, StatusCode> {
    if devices.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut requested = devices.to_vec();
    requested.sort_unstable();
    requested.dedup();
    let active = sqlx::query_scalar!(
        "SELECT id FROM device WHERE id = ANY($1) AND NOT archived ORDER BY id",
        &requested
    )
    .fetch_all(pool)
    .await
    .map_err(|err| {
        error!("Failed to look up devices to apply settings to: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    if active != requested {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(active)
}

/// Pins, defaults and unsupported settings of `device_ids`, in one query.
/// Devices without a release have no distribution, so only their pins count.
pub async fn load_settings_inputs<'e, E>(
    executor: E,
    device_ids: &[i32],
) -> Result<Vec<SettingsInputs>, sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    // Read by column name into `SettingValues`; `setting_values_match_the_tables`
    // keeps the struct and the tables in step.
    sqlx::query!(
        r#"
        SELECT d.id,
               d.settings_version,
               to_jsonb(ds) AS pins,
               to_jsonb(dsd) AS defaults,
               COALESCE(u.settings, '{}') AS "unsupported!: Vec<schema::DeviceSetting>"
        FROM device d
        LEFT JOIN release r ON r.id = d.release_id
        LEFT JOIN device_settings ds ON ds.device_id = d.id
        LEFT JOIN distribution_setting_default dsd ON dsd.distribution_id = r.distribution_id
        LEFT JOIN LATERAL (
            SELECT array_agg(setting ORDER BY setting) AS settings
            FROM distribution_unsupported_setting
            WHERE distribution_id = r.distribution_id
        ) u ON true
        WHERE d.id = ANY($1)
        ORDER BY d.id
        "#,
        device_ids
    )
    .fetch_all(executor)
    .await?
    .into_iter()
    .map(|row| {
        let values = |json: Option<serde_json::Value>| {
            json.map(serde_json::from_value::<SettingValues>)
                .transpose()
                .map(Option::unwrap_or_default)
                .map_err(|err| sqlx::Error::Decode(Box::new(err)))
        };
        Ok(SettingsInputs {
            device_id: row.id,
            settings_version: row.settings_version,
            pins: values(row.pins)?,
            defaults: values(row.defaults)?,
            unsupported: row.unsupported,
        })
    })
    .collect()
}

#[utoipa::path(
    get,
    path = "/devices/{device_id}/settings",
    params(
        ("device_id" = String, Path, description = "Device ID or serial number"),
    ),
    responses(
        (status = StatusCode::OK, description = "Settings the device reports it runs, its pins, the distribution defaults and what an Apply would send", body = DeviceSettings),
        (status = StatusCode::FORBIDDEN, description = "Missing devices:read"),
        (status = StatusCode::NOT_FOUND, description = "Device not found"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve settings"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn get_settings_for_device(
    Path(device_id): Path<String>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
) -> Result<Json<DeviceSettings>, StatusCode> {
    if !authorization::check(current_user, "devices", "read") {
        return Err(StatusCode::FORBIDDEN);
    }
    load_device_settings(&state.pg_pool, &device_id)
        .await
        .map_err(|err| {
            error!("Failed to fetch settings for device: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// `None` when no device matches `device_id` (id or serial number).
pub async fn load_device_settings(
    pool: &sqlx::PgPool,
    device_id: &str,
) -> Result<Option<DeviceSettings>, sqlx::Error> {
    let Some(device) = sqlx::query!(
        r#"
        SELECT d.id
        FROM device d
        WHERE
            CASE
                WHEN $1 ~ '^[0-9]+$' AND length($1) <= 10 THEN
                    d.id = $1::int4
                ELSE
                    d.serial_number = $1
            END
        "#,
        device_id
    )
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };

    // Read back by column name into the same struct the device sent, so the
    // column list lives only in the table (see `save_settings_report`).
    let reported = sqlx::query!(
        r#"
        SELECT to_jsonb(dsr) - 'device_id' - 'reported_at' AS "report!",
               reported_at
        FROM device_settings_reported dsr
        WHERE device_id = $1
        "#,
        device.id
    )
    .fetch_optional(pool)
    .await?
    .map(|row| {
        serde_json::from_value(row.report).map(|report| ReportedSettings {
            report,
            reported_at: row.reported_at,
        })
    })
    .transpose()
    .map_err(|err| sqlx::Error::Decode(Box::new(err)))?;

    let reported = match reported {
        Some(mut reported) => {
            reported.report.apply_status = sqlx::query!(
                r#"
                SELECT setting as "setting: schema::DeviceSetting",
                       state as "state: schema::SettingApplyState",
                       reason
                FROM device_setting_apply_status
                WHERE device_id = $1
                ORDER BY setting
                "#,
                device.id
            )
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|row| schema::SettingStatus {
                setting: row.setting,
                state: row.state,
                reason: row.reason,
            })
            .collect();
            Some(reported)
        }
        None => None,
    };

    let Some(inputs) = load_settings_inputs(pool, &[device.id]).await?.pop() else {
        return Ok(None);
    };

    Ok(Some(DeviceSettings {
        reported,
        resolved: inputs.resolve(),
        unsupported: inputs.unsupported,
        wanted: inputs.pins,
        defaults: inputs.defaults,
        settings_version: inputs.settings_version,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use smith::utils::schema::{DeviceSetting, Managed, RoamTuning, SettingsReport};
    use sqlx::PgPool;
    use std::collections::HashMap;

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn device_settings_never_reported_then_reported(pool: PgPool) -> anyhow::Result<()> {
        let distribution_id: i32 = sqlx::query_scalar(
            "INSERT INTO distribution (name) VALUES ('orin-nx-l4t-35.3.1-test') RETURNING id",
        )
        .fetch_one(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO distribution_unsupported_setting (distribution_id, setting)
             VALUES ($1, 'busy_th'), ($1, 'busy_diff')",
        )
        .bind(distribution_id)
        .execute(&pool)
        .await?;
        let release_id: i32 = sqlx::query_scalar(
            "INSERT INTO release (distribution_id, version) VALUES ($1, '1.0.0') RETURNING id",
        )
        .bind(distribution_id)
        .fetch_one(&pool)
        .await?;
        let device_id: i32 = sqlx::query_scalar(
            "INSERT INTO device (serial_number, release_id) VALUES ('settings-get', $1) RETURNING id",
        )
        .bind(release_id)
        .fetch_one(&pool)
        .await?;

        assert!(
            load_device_settings(&pool, "unknown-serial")
                .await?
                .is_none()
        );

        let settings = load_device_settings(&pool, "settings-get").await?.unwrap();
        assert!(settings.reported.is_none());
        assert_eq!(
            settings.unsupported,
            vec![DeviceSetting::BusyDiff, DeviceSetting::BusyTh]
        );

        let report = SettingsReport {
            wireless_mode: Some(95),
            read_errors: vec![DeviceSetting::Bgscan],
            ..Default::default()
        };
        crate::home::save_settings_report(&mut *pool.acquire().await?, device_id, &report).await?;
        let settings = load_device_settings(&pool, &device_id.to_string())
            .await?
            .unwrap();
        assert_eq!(settings.reported.unwrap().report, report);
        Ok(())
    }

    #[test]
    fn resolution_prefers_the_pin_then_the_default_then_unmanaged() {
        let defaults = SettingValues {
            wireless_mode: Some(11),
            power_mgnt: Some(0),
            ..Default::default()
        };
        let pins = SettingValues {
            wireless_mode: Some(95),
            hostname: Some("room-1".to_owned()),
            ..Default::default()
        };
        let resolved = resolve_settings(&pins, &defaults, &[]);
        assert_eq!(resolved.wireless_mode, Some(Managed::Set(95)), "pin");
        assert_eq!(resolved.power_mgnt, Some(Managed::Set(0)), "default");
        assert_eq!(resolved.hostname, Some(Managed::Set("room-1".to_owned())));
        assert_eq!(resolved.bgscan, Some(Managed::Unmanaged), "neither");
    }

    #[test]
    fn resolution_keeps_groups_whole_and_omits_unsupported_settings() {
        let defaults = SettingValues {
            idle_diff: Some(1),
            busy_diff: Some(2),
            scanr_exp_ms: Some(3),
            scan_int: Some(4),
            idle_th: Some(5),
            busy_th: Some(6),
            ..Default::default()
        };
        let pins = SettingValues {
            idle_diff: Some(15),
            scanr_exp_ms: Some(10000),
            scan_int: Some(5),
            idle_th: Some(30),
            ..Default::default()
        };
        let pinned = RoamTuning {
            idle_diff: 15,
            busy_diff: None,
            scanr_exp_ms: 10000,
            scan_int: 5,
            idle_th: 30,
            busy_th: None,
        };
        assert_eq!(
            resolve_settings(&pins, &defaults, &[]).roam_tuning,
            Some(Managed::Set(pinned)),
            "the default's busy fields don't leak into a pinned group"
        );

        let resolved = resolve_settings(
            &SettingValues::default(),
            &defaults,
            &[
                DeviceSetting::BusyDiff,
                DeviceSetting::BusyTh,
                DeviceSetting::Bgscan,
            ],
        );
        let Some(Managed::Set(tuning)) = resolved.roam_tuning else {
            panic!("roam tuning should be set from the default");
        };
        assert_eq!((tuning.busy_diff, tuning.busy_th), (None, None));
        assert_eq!(
            resolved.bgscan, None,
            "unsupported is left out, not unmanaged"
        );
        assert_eq!(
            resolve_settings(&pins, &defaults, &[DeviceSetting::IdleDiff]).roam_tuning,
            None
        );
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn device_settings_resolve_pins_defaults_and_unsupported(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let distribution_id: i32 =
            sqlx::query_scalar("INSERT INTO distribution (name) VALUES ('resolve') RETURNING id")
                .fetch_one(&pool)
                .await?;
        sqlx::query(
            "INSERT INTO distribution_unsupported_setting (distribution_id, setting)
             VALUES ($1, 'bgscan')",
        )
        .bind(distribution_id)
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO distribution_setting_default (distribution_id, wireless_mode, power_mgnt, bgscan)
             VALUES ($1, 11, 0, 'simple:30:-65:300')",
        )
        .bind(distribution_id)
        .execute(&pool)
        .await?;
        let release_id: i32 = sqlx::query_scalar(
            "INSERT INTO release (distribution_id, version) VALUES ($1, '1.0.0') RETURNING id",
        )
        .bind(distribution_id)
        .fetch_one(&pool)
        .await?;
        let device_id: i32 = sqlx::query_scalar(
            "INSERT INTO device (serial_number, release_id, settings_version) VALUES ('resolve', $1, 4) RETURNING id",
        )
        .bind(release_id)
        .fetch_one(&pool)
        .await?;
        sqlx::query("INSERT INTO device_settings (device_id, wireless_mode) VALUES ($1, 95)")
            .bind(device_id)
            .execute(&pool)
            .await?;

        let settings = load_device_settings(&pool, "resolve").await?.unwrap();
        assert_eq!(settings.settings_version, 4);
        assert_eq!(settings.wanted.wireless_mode, Some(95));
        assert_eq!(settings.defaults.wireless_mode, Some(11));
        assert_eq!(
            settings.resolved.power_mgnt,
            Some(Managed::Set(0)),
            "the default row reaches resolution"
        );
        assert_eq!(
            settings.resolved.bgscan, None,
            "and so do unsupported settings"
        );

        // A device without a release has no defaults: only its pins count.
        let bare: i32 =
            sqlx::query_scalar("INSERT INTO device (serial_number) VALUES ('bare') RETURNING id")
                .fetch_one(&pool)
                .await?;
        let settings = load_device_settings(&pool, &bare.to_string())
            .await?
            .unwrap();
        assert_eq!(settings.defaults, SettingValues::default());
        Ok(())
    }

    #[test]
    fn values_written_into_device_files_are_validated() {
        let valid = |changes: serde_json::Value| {
            serde_json::from_value::<SettingsChanges>(changes)
                .unwrap()
                .is_valid()
        };
        assert!(valid(json!({"hostname": {"pin": "byageren-room-12"}})));
        assert!(!valid(json!({"hostname": {"pin": "room 12"}})));
        assert!(!valid(json!({"hostname": {"pin": "-room"}})));
        assert!(!valid(json!({"hostname": {"pin": "a".repeat(65)}})));
        assert!(!valid(json!({"hostname": {"pin": "room\n127.0.0.1 api"}})));

        assert!(valid(
            json!({"ntp_servers": {"pin": ["10.122.0.1", "0.pool.ntp.org", "fe80::1"]}})
        ));
        assert!(!valid(json!({"ntp_servers": {"pin": []}})));
        assert!(!valid(
            json!({"ntp_servers": {"pin": ["pool.ntp.org\nallow all"]}})
        ));
        assert!(!valid(
            json!({"ntp_servers": {"pin": ["pool.ntp.org iburst"]}})
        ));

        let mac = |cloned_mac: &str| {
            valid(json!({"mac": {"pin": {"cloned_mac": cloned_mac, "scan_rand_mac": false}}}))
        };
        assert!(mac("preserve") && mac("stable") && mac("02:00:5e:10:00:0A"));
        assert!(!mac("preserve\n[main]") && !mac("02:00:5e:10:00") && !mac("02-00-5e-10-00-0a"));

        assert!(valid(json!({"bgscan": {"pin": "simple:30:-65:300"}})));
        assert!(
            valid(json!({"bgscan": {"pin": ""}})),
            "empty turns bgscan off"
        );
        assert!(!valid(json!({"bgscan": {"pin": "simple\"; ap_scan=0"}})));
        assert!(valid(json!({"hostname": "default"})));
    }

    #[test]
    fn changes_pin_and_reset_whole_groups() {
        let pins = SettingValues {
            wireless_mode: Some(95),
            idle_diff: Some(1),
            busy_diff: Some(2),
            scanr_exp_ms: Some(3),
            scan_int: Some(4),
            idle_th: Some(5),
            busy_th: Some(6),
            ..Default::default()
        };
        let changes: SettingsChanges = serde_json::from_value(json!({
            "wireless_mode": "default",
            "roam_tuning": {"pin": {
                "idle_diff": 15, "busy_diff": null, "scanr_exp_ms": 10000,
                "scan_int": 5, "idle_th": 30, "busy_th": null
            }},
            "mac": {"pin": {"cloned_mac": "preserve", "scan_rand_mac": false}}
        }))
        .unwrap();
        let new = changes.apply_to(&pins);
        assert_eq!(new.wireless_mode, None, "default removes the pin");
        assert_eq!(
            (new.idle_diff, new.busy_diff, new.busy_th),
            (Some(15), None, None)
        );
        assert_eq!(new.nm_cloned_mac.as_deref(), Some("preserve"));
        assert_eq!(new.nm_scan_rand_mac, Some(false));
        assert_eq!(
            changes.pinned(),
            vec![
                DeviceSetting::IdleDiff,
                DeviceSetting::ScanrExpMs,
                DeviceSetting::ScanInt,
                DeviceSetting::IdleTh,
                DeviceSetting::NmClonedMac,
                DeviceSetting::NmScanRandMac,
            ]
        );
        assert!(serde_json::from_value::<SettingsChanges>(json!({"nope": "default"})).is_err());
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn settings_edits_bump_the_version_and_queue_nothing(pool: PgPool) -> anyhow::Result<()> {
        let distribution_id: i32 =
            sqlx::query_scalar("INSERT INTO distribution (name) VALUES ('edit') RETURNING id")
                .fetch_one(&pool)
                .await?;
        sqlx::query(
            "INSERT INTO distribution_unsupported_setting (distribution_id, setting)
             VALUES ($1, 'busy_th')",
        )
        .bind(distribution_id)
        .execute(&pool)
        .await?;
        let release_id: i32 = sqlx::query_scalar(
            "INSERT INTO release (distribution_id, version) VALUES ($1, '1.0.0') RETURNING id",
        )
        .bind(distribution_id)
        .fetch_one(&pool)
        .await?;
        let mut ids = Vec::new();
        for serial in ["e1", "e2"] {
            ids.push(
                sqlx::query_scalar::<_, i32>(
                    "INSERT INTO device (serial_number, release_id) VALUES ($1, $2) RETURNING id",
                )
                .bind(serial)
                .bind(release_id)
                .fetch_one(&pool)
                .await?,
            );
        }

        let pin_mode = SettingsChanges {
            wireless_mode: Some(SettingChange::Pin(95)),
            ..Default::default()
        };
        let versions = save_settings_changes(&pool, &ids, &pin_mode, 7).await?;
        assert!(versions.iter().all(|v| v.settings_version == 1));
        // The same edit again changes nothing, so the version stays.
        let versions = save_settings_changes(&pool, &ids, &pin_mode, 7).await?;
        assert!(versions.iter().all(|v| v.settings_version == 1));

        let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM command_queue")
            .fetch_one(&pool)
            .await?;
        assert_eq!(queued, 0, "an edit queues nothing");

        let busy_th = SettingsChanges {
            roam_tuning: Some(SettingChange::Pin(RoamTuning {
                idle_diff: 15,
                busy_diff: Some(15),
                scanr_exp_ms: 10000,
                scan_int: 5,
                idle_th: 30,
                busy_th: Some(30),
            })),
            ..Default::default()
        };
        assert!(matches!(
            save_settings_changes(&pool, &ids, &busy_th, 7).await,
            Err(SettingsEditError::Unsupported)
        ));
        assert!(matches!(
            save_settings_changes(&pool, &[ids[0], i32::MAX], &pin_mode, 7).await,
            Err(SettingsEditError::NotFound)
        ));

        let hostname = SettingsChanges {
            hostname: Some(SettingChange::Pin("room-1".to_owned())),
            ..Default::default()
        };
        assert!(
            matches!(
                save_settings_changes(&pool, &ids, &hostname, 7).await,
                Err(SettingsEditError::Invalid)
            ),
            "one hostname on several devices"
        );
        save_settings_changes(&pool, &ids[..1], &hostname, 7).await?;
        sqlx::query("UPDATE device SET archived = true WHERE id = $1")
            .bind(ids[1])
            .execute(&pool)
            .await?;
        assert!(matches!(
            save_settings_changes(&pool, &ids[1..], &pin_mode, 7).await,
            Err(SettingsEditError::NotFound)
        ));

        // Back to default on the only pin removes the row and still counts as
        // an edit.
        let reset = SettingsChanges {
            wireless_mode: Some(SettingChange::Default),
            ..Default::default()
        };
        let versions = save_settings_changes(&pool, &ids[..1], &reset, 7).await?;
        assert_eq!(versions[0].settings_version, 3);
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM device_settings")
            .fetch_one(&pool)
            .await?;
        assert_eq!(rows, 2, "the hostname pin keeps the first row");
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn settings_apply_queues_one_bundle_with_the_resolved_sets(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let user_id: i32 = sqlx::query_scalar(
            "INSERT INTO auth.users (auth0_user_id, email) VALUES ('apply', 'apply@test') RETURNING id",
        )
        .fetch_one(&pool)
        .await?;
        let distribution_id: i32 =
            sqlx::query_scalar("INSERT INTO distribution (name) VALUES ('apply') RETURNING id")
                .fetch_one(&pool)
                .await?;
        sqlx::query(
            "INSERT INTO distribution_setting_default (distribution_id, wireless_mode)
             VALUES ($1, 11)",
        )
        .bind(distribution_id)
        .execute(&pool)
        .await?;
        let release_id: i32 = sqlx::query_scalar(
            "INSERT INTO release (distribution_id, version) VALUES ($1, '1.0.0') RETURNING id",
        )
        .bind(distribution_id)
        .fetch_one(&pool)
        .await?;
        let ids: Vec<i32> = sqlx::query_scalar(
            "INSERT INTO device (serial_number, release_id, settings_version)
             SELECT 'apply-' || n, $1, 1 FROM generate_series(1, 3) n
             RETURNING id",
        )
        .bind(release_id)
        .fetch_all(&pool)
        .await?;
        // Nothing to apply on this one.
        sqlx::query("UPDATE device SET settings_version = 0 WHERE id = $1")
            .bind(ids[0])
            .execute(&pool)
            .await?;
        assert_eq!(
            queue_settings_apply(&pool, &ids[..1], ApplyBy::User(user_id))
                .await
                .map(|r| r.is_none()),
            Ok(true)
        );

        let receipt = queue_settings_apply(&pool, &ids, ApplyBy::User(user_id))
            .await
            .unwrap()
            .unwrap();
        let rows: Vec<(i32, serde_json::Value)> = sqlx::query_as(
            "SELECT device_id, cmd::jsonb FROM command_queue WHERE bundle = $1 ORDER BY device_id",
        )
        .bind(receipt.uuid)
        .fetch_all(&pool)
        .await?;
        assert_eq!(
            rows.iter().map(|(device, _)| *device).collect::<Vec<_>>(),
            ids[1..],
            "one per device in one bundle, version 0 skipped"
        );
        assert_eq!(rows[0].1["ApplySettings"]["version"], 1);
        assert_eq!(
            rows[0].1["ApplySettings"]["settings"]["wireless_mode"]["Set"],
            11
        );
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn new_devices_get_their_defaults_once_without_an_apply(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let distribution_id: i32 =
            sqlx::query_scalar("INSERT INTO distribution (name) VALUES ('auto') RETURNING id")
                .fetch_one(&pool)
                .await?;
        let release_id: i32 = sqlx::query_scalar(
            "INSERT INTO release (distribution_id, version) VALUES ($1, '1.0.0') RETURNING id",
        )
        .bind(distribution_id)
        .fetch_one(&pool)
        .await?;
        let device_id: i32 = sqlx::query_scalar(
            "INSERT INTO device (serial_number, release_id) VALUES ('new', $1) RETURNING id",
        )
        .bind(release_id)
        .fetch_one(&pool)
        .await?;
        // Like every device after the launch migration.
        let existing: i32 = sqlx::query_scalar(
            "INSERT INTO device (serial_number, release_id, settings_version)
             VALUES ('existing', $1, 1) RETURNING id",
        )
        .bind(release_id)
        .fetch_one(&pool)
        .await?;
        let applies = || {
            let pool = pool.clone();
            async move {
                sqlx::query_as::<_, (serde_json::Value,)>(
                    "SELECT cmd::jsonb FROM command_queue WHERE device_id = $1",
                )
                .bind(device_id)
                .fetch_all(&pool)
                .await
            }
        };

        // No defaults, then a defaults row that manages nothing: nothing to send.
        auto_apply_settings(&pool, device_id).await.unwrap();
        sqlx::query("INSERT INTO distribution_setting_default (distribution_id) VALUES ($1)")
            .bind(distribution_id)
            .execute(&pool)
            .await?;
        auto_apply_settings(&pool, device_id).await.unwrap();
        assert!(applies().await?.is_empty());

        sqlx::query(
            "UPDATE distribution_setting_default SET wireless_mode = 11 WHERE distribution_id = $1",
        )
        .bind(distribution_id)
        .execute(&pool)
        .await?;
        auto_apply_settings(&pool, device_id).await.unwrap();
        auto_apply_settings(&pool, device_id).await.unwrap();
        auto_apply_settings(&pool, existing).await.unwrap();
        let existing_commands: i64 =
            sqlx::query_scalar("SELECT count(*) FROM command_queue WHERE device_id = $1")
                .bind(existing)
                .fetch_one(&pool)
                .await?;
        assert_eq!(
            existing_commands, 0,
            "a device past version 0 waits for an Apply"
        );
        let sent = applies().await?;
        assert_eq!(sent.len(), 1, "sent once");
        assert_eq!(sent[0].0["ApplySettings"]["version"], 1);
        assert_eq!(
            sent[0].0["ApplySettings"]["settings"]["wireless_mode"]["Set"],
            11
        );
        let author: Option<i32> = sqlx::query_scalar(
            "SELECT b.user_id FROM command_bundles b JOIN command_queue q ON q.bundle = b.uuid
             WHERE q.device_id = $1",
        )
        .bind(device_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(author, None, "a system bundle");

        // A poll that saw version 0 just before another one claimed it queues
        // nothing: the device is at 1 now.
        assert_eq!(
            queue_settings_apply(&pool, &[device_id], ApplyBy::AutoApply)
                .await
                .map(|receipt| receipt.is_none()),
            Ok(true)
        );
        assert_eq!(applies().await?.len(), 1);
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn settings_apply_takes_only_known_active_devices(pool: PgPool) -> anyhow::Result<()> {
        use axum::http::StatusCode;

        let mut ids = HashMap::new();
        for (serial, archived) in [("a", false), ("b", false), ("d", true)] {
            let id: i32 = sqlx::query_scalar(
                "INSERT INTO device (serial_number, archived) VALUES ($1, $2) RETURNING id",
            )
            .bind(serial)
            .bind(archived)
            .fetch_one(&pool)
            .await?;
            ids.insert(serial, id);
        }
        assert_eq!(
            active_devices(&pool, &[ids["b"], ids["a"], ids["a"]]).await,
            Ok(vec![ids["a"], ids["b"]])
        );
        assert_eq!(
            active_devices(&pool, &[ids["a"], ids["d"]]).await,
            Err(StatusCode::NOT_FOUND),
            "archived"
        );
        assert_eq!(
            active_devices(&pool, &[ids["a"], i32::MAX]).await,
            Err(StatusCode::NOT_FOUND),
            "unknown"
        );
        assert_eq!(
            active_devices(&pool, &[]).await,
            Err(StatusCode::BAD_REQUEST)
        );
        Ok(())
    }

    /// The upsert and the audit trigger each list the columns again; an edit of
    /// every field must reach both, with the api user as the author.
    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn every_field_is_written_and_audited(pool: PgPool) -> anyhow::Result<()> {
        let device_id: i32 =
            sqlx::query_scalar("INSERT INTO device (serial_number) VALUES ('all') RETURNING id")
                .fetch_one(&pool)
                .await?;
        let every_field = |n: i32, name: &str| -> anyhow::Result<SettingsChanges> {
            Ok(serde_json::from_value(json!({
                "wireless_mode": {"pin": n},
                "power_mgnt": {"pin": n + 1},
                "roam_flags": {"pin": n + 2},
                "roam_tuning": {"pin": {
                    "idle_diff": n + 3, "busy_diff": n + 4, "scanr_exp_ms": n + 5,
                    "scan_int": n + 6, "idle_th": n + 7, "busy_th": n + 8
                }},
                "nm_wifi_powersave": {"pin": n + 9},
                "nm_autoconnect_retries": {"pin": n + 10},
                "mac": {"pin": {"cloned_mac": name, "scan_rand_mac": n % 2 == 0}},
                "hostname": {"pin": format!("host-{n}")},
                "ntp_servers": {"pin": [format!("{n}.pool.ntp.org")]},
                "bgscan": {"pin": format!("simple:{n}:-65:300")}
            }))?)
        };
        let read = |query: &'static str| {
            let pool = pool.clone();
            async move {
                let json: serde_json::Value = sqlx::query_scalar(query)
                    .bind(device_id)
                    .fetch_one(&pool)
                    .await?;
                anyhow::Ok(serde_json::from_value::<SettingValues>(json)?)
            }
        };

        for (n, cloned_mac, operation) in [(1, "preserve", "INSERT"), (20, "stable", "UPDATE")] {
            let changes = every_field(n, cloned_mac)?;
            let expected = changes.apply_to(&SettingValues::default());
            assert!(expected.busy_th.is_some() && expected.bgscan.is_some());
            save_settings_changes(&pool, &[device_id], &changes, 7).await?;
            assert_eq!(
                read("SELECT to_jsonb(ds) FROM device_settings ds WHERE device_id = $1").await?,
                expected,
                "row after edit {n}"
            );
            assert_eq!(
                read(
                    "SELECT to_jsonb(a) FROM device_settings_audit a
                     WHERE device_id = $1 ORDER BY audit_id DESC LIMIT 1"
                )
                .await?,
                expected,
                "audit after edit {n}"
            );
            let (changed_by, op): (Option<i32>, String) = sqlx::query_as(
                "SELECT changed_by, operation FROM device_settings_audit
                 WHERE device_id = $1 ORDER BY audit_id DESC LIMIT 1",
            )
            .bind(device_id)
            .fetch_one(&pool)
            .await?;
            assert_eq!((changed_by, op.as_str()), (Some(7), operation));
        }
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn setting_values_match_the_tables(pool: PgPool) -> anyhow::Result<()> {
        use std::collections::BTreeSet;

        let fields: BTreeSet<String> = serde_json::to_value(SettingValues::default())?
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        for (table, own) in [
            (
                "device_settings",
                &["device_id", "updated_at", "updated_by"][..],
            ),
            ("distribution_setting_default", &["distribution_id"][..]),
            (
                "device_settings_audit",
                &[
                    "audit_id",
                    "changed_at",
                    "changed_by",
                    "operation",
                    "device_id",
                ][..],
            ),
        ] {
            let columns: BTreeSet<String> = sqlx::query_scalar(
                "SELECT column_name::text FROM information_schema.columns WHERE table_name = $1",
            )
            .bind(table)
            .fetch_all(&pool)
            .await?
            .into_iter()
            .filter(|column: &String| !own.contains(&column.as_str()))
            .collect();
            assert_eq!(fields, columns, "SettingValues and {table} differ");
        }
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn device_settings_checks_keep_groups_whole(pool: PgPool) -> anyhow::Result<()> {
        let device_id: i32 =
            sqlx::query_scalar("INSERT INTO device (serial_number) VALUES ('c') RETURNING id")
                .fetch_one(&pool)
                .await?;
        let insert = |columns: &str, values: &str| {
            format!("INSERT INTO device_settings (device_id, {columns}) VALUES ($1, {values})")
        };
        let rejected = [
            ("idle_diff", "15"),
            ("idle_diff, scanr_exp_ms, scan_int", "15, 10000, 5"),
            ("busy_diff", "15"),
            ("nm_cloned_mac", "'preserve'"),
            ("updated_by", "1"),
        ];
        for (columns, values) in rejected {
            assert!(
                sqlx::query(&insert(columns, values))
                    .bind(device_id)
                    .execute(&pool)
                    .await
                    .is_err(),
                "{columns} should be rejected"
            );
        }
        // The busy fields may stay NULL inside a set roam group.
        sqlx::query(&insert(
            "idle_diff, scanr_exp_ms, scan_int, idle_th, nm_cloned_mac, nm_scan_rand_mac",
            "15, 10000, 5, 30, 'preserve', false",
        ))
        .bind(device_id)
        .execute(&pool)
        .await?;

        let distribution_id: i32 = sqlx::query_scalar(
            "INSERT INTO distribution (name) VALUES ('checks-test') RETURNING id",
        )
        .fetch_one(&pool)
        .await?;
        for (columns, values) in [("nm_scan_rand_mac", "false"), ("scan_int", "5")] {
            assert!(
                sqlx::query(&format!(
                    "INSERT INTO distribution_setting_default (distribution_id, {columns}) VALUES ($1, {values})"
                ))
                .bind(distribution_id)
                .execute(&pool)
                .await
                .is_err(),
                "default {columns} should be rejected"
            );
        }
        // A default row may be empty: nothing managed.
        sqlx::query("INSERT INTO distribution_setting_default (distribution_id) VALUES ($1)")
            .bind(distribution_id)
            .execute(&pool)
            .await?;
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn audit_keeps_changes_made_outside_the_api_and_removed_rows(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let device_id: i32 =
            sqlx::query_scalar("INSERT INTO device (serial_number) VALUES ('a') RETURNING id")
                .fetch_one(&pool)
                .await?;
        // Like a migration: no smith.user_id set.
        sqlx::query("INSERT INTO device_settings (device_id, wireless_mode) VALUES ($1, 95)")
            .bind(device_id)
            .execute(&pool)
            .await?;
        sqlx::query("DELETE FROM device_settings WHERE device_id = $1")
            .bind(device_id)
            .execute(&pool)
            .await?;

        let rows: Vec<(Option<i32>, String, Option<i16>)> = sqlx::query_as(
            "SELECT changed_by, operation, wireless_mode FROM device_settings_audit
             WHERE device_id = $1 ORDER BY audit_id",
        )
        .bind(device_id)
        .fetch_all(&pool)
        .await?;
        assert_eq!(
            rows,
            vec![
                (None, "INSERT".to_owned(), Some(95)),
                (None, "DELETE".to_owned(), Some(95)),
            ]
        );
        Ok(())
    }
}
