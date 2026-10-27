//! Device settings: what devices report, the pins and distribution defaults
//! they should run, and the edit and Apply endpoints that send them.

use super::route::DEVICES_TAG;
use crate::State;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::{Extension, Json};
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
}

#[utoipa::path(
    get,
    path = "/devices/{device_id}/settings",
    params(
        ("device_id" = String, Path, description = "Device ID or serial number"),
    ),
    responses(
        (status = StatusCode::OK, description = "Settings the device reports it runs", body = DeviceSettings),
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
) -> Result<Json<DeviceSettings>, StatusCode> {
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
        SELECT d.id, r.distribution_id as "distribution_id?"
        FROM device d
        LEFT JOIN release r ON r.id = d.release_id
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

    let unsupported = match device.distribution_id {
        Some(distribution_id) => {
            sqlx::query_scalar!(
                r#"
            SELECT setting as "setting: schema::DeviceSetting"
            FROM distribution_unsupported_setting
            WHERE distribution_id = $1
            ORDER BY setting
            "#,
                distribution_id
            )
            .fetch_all(pool)
            .await?
        }
        None => Vec::new(),
    };

    Ok(Some(DeviceSettings {
        reported,
        unsupported,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use smith::utils::schema::{DeviceSetting, SettingsReport};
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
