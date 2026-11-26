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
            WHERE CARDINALITY($1::text[]) > 0
              AND dl.device_id = d.id
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
    pub async fn new(pool: &PgPool, excluded_labels: &[String]) -> Result<Self, sqlx::Error> {
        // Keep CARDINALITY($1) > 0 inside NOT EXISTS so empty arrays skip the
        // anti-join via a one-time filter while non-empty arrays still use Hash Anti Join.
        let row = sqlx::query!(
            r#"
            SELECT
                COUNT(*) FILTER (WHERE d.archived = false) AS "total_count!",
                COUNT(*) FILTER (
                    WHERE d.archived = false
                      AND d.last_ping >= now() - INTERVAL '3 minutes'
                ) AS "online_count!",
                COUNT(*) FILTER (
                    WHERE d.archived = false
                      AND d.last_ping < now() - INTERVAL '3 minutes'
                ) AS "offline_count!",
                COUNT(*) FILTER (
                    WHERE d.archived = false
                      AND d.release_id != d.target_release_id
                      AND NOT COALESCE(d.target_release_eligible_at > now(), false)
                ) AS "outdated_count!",
                COUNT(*) FILTER (WHERE d.archived = true) AS "archived_count!"
            FROM device d
            WHERE NOT EXISTS (
                SELECT 1 FROM device_label dl
                JOIN label l ON l.id = dl.label_id
                WHERE CARDINALITY($1::text[]) > 0
                  AND dl.device_id = d.id
                  AND l.name || '=' || dl.value = ANY($1)
            )
            "#,
            excluded_labels
        )
        .fetch_one(pool)
        .await?;

        Ok(Self {
            total_count: u32::try_from(row.total_count).unwrap_or(0),
            online_count: u32::try_from(row.online_count).unwrap_or(0),
            offline_count: u32::try_from(row.offline_count).unwrap_or(0),
            outdated_count: u32::try_from(row.outdated_count).unwrap_or(0),
            archived_count: u32::try_from(row.archived_count).unwrap_or(0),
        })
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
              WHERE CARDINALITY($1::text[]) > 0
                AND dl.device_id = d.id
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
