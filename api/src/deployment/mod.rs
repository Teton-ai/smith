use models::deployment::Deployment;
use models::deployment::DeploymentRequest;
use models::deployment::DeploymentStatus;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use sqlx::types::chrono;
use utoipa::ToSchema;

use crate::error::ApiError;

pub mod route;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct ServiceStatusInfo {
    pub name: String,
    pub active: bool,
    pub uptime_sec: Option<u64>,
    pub healthy: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct DeploymentDeviceWithStatus {
    pub device_id: i32,
    pub serial_number: String,
    pub release_id: Option<i32>,
    pub target_release_id: Option<i32>,
    pub last_ping: Option<chrono::DateTime<chrono::Utc>>,
    pub added_at: chrono::DateTime<chrono::Utc>,
    pub services_healthy: Option<bool>,
    #[schema(value_type = Option<Vec<ServiceStatusInfo>>)]
    pub services_status: Option<Value>,
}

pub async fn get_deployment(
    release_id: i32,
    pg_pool: &PgPool,
) -> anyhow::Result<Option<Deployment>> {
    Ok(sqlx::query_as!(
        Deployment,
        r#"
            SELECT id, release_id, status AS "status!: DeploymentStatus", updated_at, created_at
            FROM deployment WHERE release_id = $1
            "#,
        release_id
    )
    .fetch_optional(pg_pool)
    .await?)
}

pub async fn new_deployment(
    release_id: i32,
    request: Option<DeploymentRequest>,
    pg_pool: &PgPool,
) -> Result<Deployment, ApiError> {
    let mut tx = pg_pool.begin().await?;
    // Get the distribution_id for this release
    let release = sqlx::query!(
        "SELECT distribution_id FROM release WHERE id = $1",
        release_id
    )
    .fetch_one(&mut *tx)
    .await?;

    let deployment = sqlx::query_as!(
        Deployment,
        r#"
        INSERT INTO deployment (release_id, status)
        VALUES ($1, 'in_progress')
        RETURNING id, release_id, status AS "status!: DeploymentStatus", updated_at, created_at
        "#,
        release_id
    )
    .fetch_one(&mut *tx)
    .await?;

    let res = if let Some(canary_device_labels) = request.and_then(|req| req.canary_device_labels)
        && !canary_device_labels.is_empty()
    {
        sqlx::query!(
            r#"
            WITH selected_devices AS (
                SELECT DISTINCT d.id FROM device d
                JOIN release r ON d.release_id = r.id
                LEFT JOIN device_label dl ON dl.device_id = d.id
                LEFT JOIN label l ON l.id = dl.label_id
                WHERE
                    l.name || '=' || dl.value = ANY($3)
                    AND d.release_id = d.target_release_id
                    AND r.distribution_id = $1
            )
            INSERT INTO deployment_devices (deployment_id, device_id)
            SELECT $2, id FROM selected_devices
            
            "#,
            release.distribution_id,
            deployment.id,
            canary_device_labels.as_slice()
        )
        .execute(&mut *tx)
        .await?
    } else {
        sqlx::query!(
            "
            WITH selected_devices AS (
                SELECT d.id FROM device d
                JOIN release r ON d.release_id = r.id
                LEFT JOIN device_network dn ON d.id = dn.device_id
                WHERE d.last_ping > NOW() - INTERVAL '5 minutes'
                AND d.release_id = d.target_release_id
                AND r.distribution_id = $1
                ORDER BY
                    COALESCE(dn.network_score, 0) DESC,
                    d.last_ping DESC
                LIMIT 10
            )
            INSERT INTO deployment_devices (deployment_id, device_id)
            SELECT $2, id FROM selected_devices
            ",
            release.distribution_id,
            deployment.id
        )
        .execute(&mut *tx)
        .await?
    };
    if res.rows_affected() == 0 {
        tx.rollback().await?;
        return Err(ApiError::bad_request(
            "Canary release contains no devices, aborting.",
        ));
    }

    sqlx::query!(
        "
        UPDATE device
        SET target_release_id = $1
        WHERE id IN (
            SELECT device_id FROM deployment_devices WHERE deployment_id = $2
        )
        ",
        release_id,
        deployment.id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(deployment)
}

pub async fn confirm_full_rollout(release_id: i32, pg_pool: &PgPool) -> anyhow::Result<Deployment> {
    let mut tx = pg_pool.begin().await?;

    let deployment = sqlx::query!(
            "SELECT id, release_id, status AS \"status!: DeploymentStatus\" FROM deployment WHERE release_id = $1",
            release_id
        )
        .fetch_one(&mut *tx)
        .await?;

    if deployment.status == DeploymentStatus::Done {
        let deployment_obj = sqlx::query_as!(
            Deployment,
            "SELECT id, release_id, status AS \"status!: DeploymentStatus\", updated_at, created_at
                 FROM deployment WHERE id = $1",
            deployment.id
        )
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        return Ok(deployment_obj);
    }

    let release = sqlx::query!(
        "SELECT distribution_id FROM release WHERE id = $1",
        release_id
    )
    .fetch_one(&mut *tx)
    .await?;

    let deployment_devices = sqlx::query!(
        "SELECT device_id
             FROM deployment_devices
             WHERE deployment_id = $1",
        deployment.id
    )
    .fetch_all(&mut *tx)
    .await?;

    let device_ids: Vec<i32> = deployment_devices.iter().map(|dd| dd.device_id).collect();

    if device_ids.is_empty() {
        anyhow::bail!("No canary devices found in deployment");
    }

    // Check: All devices must have release_id == target_release_id
    // Service health is informational only - doesn't block rollout
    let mismatched_devices_count = sqlx::query_scalar!(
        "SELECT COUNT(*)
             FROM device
             WHERE id = ANY($1) AND release_id != target_release_id",
        &device_ids
    )
    .fetch_one(&mut *tx)
    .await?;

    if mismatched_devices_count.unwrap_or(0) > 0 {
        anyhow::bail!("Cannot confirm full rollout: canary devices have not completed updating");
    }

    let updated_deployment = sqlx::query_as!(
            Deployment,
            "UPDATE deployment SET status = 'done'
             WHERE release_id = $1
             RETURNING id, release_id, status AS \"status!: DeploymentStatus\", updated_at, created_at",
            release_id
        )
        .fetch_one(&mut *tx)
        .await?;

    sqlx::query_scalar!(
        "SELECT COUNT(*)
             FROM device
             WHERE device.release_id IN (
                SELECT id FROM release WHERE distribution_id = $1
             )",
        release.distribution_id
    )
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query!(
        "UPDATE device
             SET target_release_id = $1
             WHERE device.release_id IN (
                SELECT id FROM release WHERE distribution_id = $2
             )",
        release_id,
        release.distribution_id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(updated_deployment)
}

pub async fn get_devices_in_deployment(
    release_id: i32,
    pg_pool: &PgPool,
) -> anyhow::Result<Vec<DeploymentDeviceWithStatus>> {
    let deployment = sqlx::query!(
        "SELECT id FROM deployment WHERE release_id = $1",
        release_id
    )
    .fetch_optional(pg_pool)
    .await?;

    let Some(deployment) = deployment else {
        return Ok(Vec::new());
    };

    // Get services that need monitoring for this release
    let required_services: Vec<String> = sqlx::query_scalar!(
        "SELECT service_name FROM release_services WHERE release_id = $1 AND watchdog_sec IS NOT NULL",
        release_id
    )
    .fetch_all(pg_pool)
    .await?;

    // Raw query result struct
    struct DeviceRow {
        device_id: i32,
        serial_number: String,
        release_id: Option<i32>,
        target_release_id: Option<i32>,
        last_ping: Option<chrono::DateTime<chrono::Utc>>,
        added_at: chrono::DateTime<chrono::Utc>,
        services_status: Option<Value>,
    }

    let rows = sqlx::query_as!(
        DeviceRow,
        r#"
            SELECT
                d.id AS device_id,
                d.serial_number,
                d.release_id,
                d.target_release_id,
                d.last_ping,
                dd.created_at AS added_at,
                d.system_info->'services_status' AS services_status
            FROM deployment_devices dd
            JOIN device d ON dd.device_id = d.id
            WHERE dd.deployment_id = $1
            ORDER BY dd.created_at ASC
            "#,
        deployment.id
    )
    .fetch_all(pg_pool)
    .await?;

    let devices = rows
        .into_iter()
        .map(|row| {
            let services_healthy = compute_services_healthy(&row.services_status, &required_services);
            DeploymentDeviceWithStatus {
                device_id: row.device_id,
                serial_number: row.serial_number,
                release_id: row.release_id,
                target_release_id: row.target_release_id,
                last_ping: row.last_ping,
                added_at: row.added_at,
                services_healthy,
                services_status: row.services_status,
            }
        })
        .collect();

    Ok(devices)
}

/// Check if all required services are healthy based on the device's services_status
fn compute_services_healthy(
    services_status: &Option<Value>,
    required_services: &[String],
) -> Option<bool> {
    // If no services to monitor, return None (not applicable)
    if required_services.is_empty() {
        return None;
    }

    // If we have required services but no status, they're not healthy
    let Some(status_value) = services_status else {
        return Some(false);
    };

    // Parse the services status array
    let Some(status_array) = status_value.as_array() else {
        return Some(false);
    };

    // Check each required service
    for required_service in required_services {
        let service_status = status_array.iter().find(|s| {
            s.get("name")
                .and_then(|n| n.as_str())
                .map(|n| n == required_service)
                .unwrap_or(false)
        });

        match service_status {
            Some(status) => {
                let healthy = status.get("healthy").and_then(|h| h.as_bool()).unwrap_or(false);
                if !healthy {
                    return Some(false);
                }
            }
            None => {
                // Required service not found in status
                return Some(false);
            }
        }
    }

    Some(true)
}
