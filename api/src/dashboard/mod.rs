use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;

pub mod route;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct RegistrationCount {
    pub date: NaiveDate,
    pub count: i64,
}

pub async fn get_registration_counts(
    pool: &PgPool,
    excluded_labels: &[String],
) -> Result<Vec<RegistrationCount>, sqlx::Error> {
    sqlx::query_as!(
        RegistrationCount,
        r#"
        SELECT DATE_TRUNC('month', created_on)::date as "date!", COUNT(*) as "count!"
        FROM device d
        WHERE NOT EXISTS (
            SELECT 1 FROM device_label dl
            JOIN label l ON l.id = dl.label_id
            WHERE dl.device_id = d.id
            AND l.name || '=' || dl.value = ANY($1)
        )
        GROUP BY DATE_TRUNC('month', created_on)
        ORDER BY DATE_TRUNC('month', created_on)
        "#,
        excluded_labels
    )
    .fetch_all(pool)
    .await
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct Dashboard {
    pub total_count: u32,
    pub online_count: u32,
    pub offline_count: u32,
    pub outdated_count: u32,
    pub archived_count: u32,
}

impl Dashboard {
    pub async fn new(pool: &PgPool, excluded_labels: &[String]) -> Self {
        let total_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) FROM device d
            WHERE d.archived = false
            AND NOT EXISTS (
                SELECT 1 FROM device_label dl
                JOIN label l ON l.id = dl.label_id
                WHERE dl.device_id = d.id
                AND l.name || '=' || dl.value = ANY($1)
            )
            "#,
            excluded_labels
        )
        .fetch_one(pool)
        .await
        .unwrap_or(Some(0))
        .unwrap_or(0) as u32;

        let online_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) FROM device d
            WHERE d.last_ping >= now() - INTERVAL '3 minutes'
            AND d.archived = false
            AND NOT EXISTS (
                SELECT 1 FROM device_label dl
                JOIN label l ON l.id = dl.label_id
                WHERE dl.device_id = d.id
                AND l.name || '=' || dl.value = ANY($1)
            )
            "#,
            excluded_labels
        )
        .fetch_one(pool)
        .await
        .unwrap_or(Some(0))
        .unwrap_or(0) as u32;

        let offline_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) FROM device d
            WHERE d.last_ping < now() - INTERVAL '3 minutes'
            AND d.archived = false
            AND NOT EXISTS (
                SELECT 1 FROM device_label dl
                JOIN label l ON l.id = dl.label_id
                WHERE dl.device_id = d.id
                AND l.name || '=' || dl.value = ANY($1)
            )
            "#,
            excluded_labels
        )
        .fetch_one(pool)
        .await
        .unwrap_or(Some(0))
        .unwrap_or(0) as u32;

        let outdated_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) FROM device d
            WHERE d.release_id != d.target_release_id
            AND d.archived = false
            AND NOT EXISTS (
                SELECT 1 FROM device_label dl
                JOIN label l ON l.id = dl.label_id
                WHERE dl.device_id = d.id
                AND l.name || '=' || dl.value = ANY($1)
            )
            "#,
            excluded_labels
        )
        .fetch_one(pool)
        .await
        .unwrap_or(Some(0))
        .unwrap_or(0) as u32;

        let archived_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) FROM device d
            WHERE d.archived = true
            AND NOT EXISTS (
                SELECT 1 FROM device_label dl
                JOIN label l ON l.id = dl.label_id
                WHERE dl.device_id = d.id
                AND l.name || '=' || dl.value = ANY($1)
            )
            "#,
            excluded_labels
        )
        .fetch_one(pool)
        .await
        .unwrap_or(Some(0))
        .unwrap_or(0) as u32;

        Self {
            total_count,
            online_count,
            offline_count,
            outdated_count,
            archived_count,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct UnhealthyServiceDevice {
    pub device_id: i32,
    pub serial_number: String,
    pub service_name: String,
    pub active_state: String,
    pub n_restarts: i32,
    pub checked_at: DateTime<Utc>,
}

pub async fn get_unhealthy_service_devices(
    pg_pool: &PgPool,
    excluded_labels: &[String],
) -> anyhow::Result<Vec<UnhealthyServiceDevice>> {
    let devices = sqlx::query_as!(
        UnhealthyServiceDevice,
        r#"
        SELECT
            d.id AS device_id,
            d.serial_number,
            rs.service_name,
            dss.active_state,
            dss.n_restarts,
            dss.checked_at
        FROM device d
        JOIN device_service_status dss ON dss.device_id = d.id
        JOIN release_services rs ON rs.id = dss.release_service_id
        WHERE d.last_ping >= now() - INTERVAL '3 minutes'
          AND d.archived = false
          AND (d.release_id = d.target_release_id OR d.target_release_id IS NULL)
          AND rs.release_id = d.release_id
          AND rs.watchdog_sec IS NOT NULL
          AND dss.active_state != 'active'
          AND NOT EXISTS (
              SELECT 1 FROM device_label dl
              JOIN label l ON l.id = dl.label_id
              WHERE dl.device_id = d.id
              AND l.name || '=' || dl.value = ANY($1)
          )
        ORDER BY d.serial_number, rs.service_name
        "#,
        excluded_labels
    )
    .fetch_all(pg_pool)
    .await?;

    Ok(devices)
}
