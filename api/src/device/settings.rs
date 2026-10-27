//! Device settings: what devices report, the pins and distribution defaults
//! they should run, and the edit and Apply endpoints that send them.

use super::route::DEVICES_TAG;
use crate::State;
use crate::middlewares::authorization;
use crate::user::CurrentUser;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::Deserialize;
use smith::utils::schema;
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
    /// The device answered its latest `ApplySettings` as a command it does not
    /// know.
    pub needs_newer_smithd: bool,
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
        SELECT d.id, d.settings_needs_newer_smithd
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
        needs_newer_smithd: device.settings_needs_newer_smithd,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use smith::utils::schema::{DeviceSetting, Managed, RoamTuning, SettingsReport};
    use sqlx::PgPool;

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
