use std::collections::HashMap;

use chrono::{DateTime, Utc};
use models::deployment::Deployment;
use models::deployment::DeploymentRequest;
use models::deployment::DeploymentStatus;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use sqlx::types::Json as SqlxJson;
use sqlx::types::chrono;
use utoipa::ToSchema;

use crate::config::Config;
use crate::error::ApiError;
use crate::slack::send_slack_notification;

pub mod route;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct DeploymentDeviceWithStatus {
    pub device_id: i32,
    pub serial_number: String,
    pub release_id: Option<i32>,
    pub target_release_id: Option<i32>,
    pub last_ping: Option<chrono::DateTime<chrono::Utc>>,
    pub added_at: chrono::DateTime<chrono::Utc>,
    #[schema(value_type = HashMap<String, String>)]
    pub labels: SqlxJson<HashMap<String, String>>,
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
    config: &Config,
    user_email: Option<&str>,
) -> Result<Deployment, ApiError> {
    let mut tx = pg_pool.begin().await?;
    let release = sqlx::query!(
        "SELECT distribution_id, draft, yanked FROM release WHERE id = $1",
        release_id
    )
    .fetch_one(&mut *tx)
    .await?;

    if release.draft {
        return Err(ApiError::bad_request("Cannot deploy a draft release."));
    }
    if release.yanked {
        return Err(ApiError::bad_request("Cannot deploy a yanked release."));
    }

    // A release keeps a single deployment row, so deploying one that already
    // finished (e.g. going back to an older version) restarts that row with a
    // fresh canary instead of tripping the unique constraint.
    let deployment = sqlx::query_as!(
        Deployment,
        r#"
        INSERT INTO deployment (release_id, status)
        VALUES ($1, 'in_progress')
        ON CONFLICT (release_id) DO UPDATE
            SET status = 'in_progress', created_at = NOW(), updated_at = NOW()
            WHERE deployment.status <> 'in_progress'
        RETURNING id, release_id, status AS "status!: DeploymentStatus", updated_at, created_at
        "#,
        release_id
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(deployment) = deployment else {
        return Err(ApiError::bad_request(
            "A deployment for this release is already in progress.",
        ));
    };

    sqlx::query!(
        "DELETE FROM deployment_devices WHERE deployment_id = $1",
        deployment.id
    )
    .execute(&mut *tx)
    .await?;

    let labels_opt = request
        .as_ref()
        .and_then(|r| r.canary_device_labels.as_ref());
    let ids_opt = request.as_ref().and_then(|r| r.canary_device_ids.as_ref());

    if matches!(labels_opt, Some(l) if l.is_empty()) {
        tx.rollback().await?;
        return Err(ApiError::bad_request(
            "canary_device_labels cannot be empty.",
        ));
    }
    if matches!(ids_opt, Some(i) if i.is_empty()) {
        tx.rollback().await?;
        return Err(ApiError::bad_request("canary_device_ids cannot be empty."));
    }

    let has_labels = labels_opt.is_some_and(|l| !l.is_empty());
    let has_ids = ids_opt.is_some_and(|i| !i.is_empty());

    if has_labels && has_ids {
        tx.rollback().await?;
        return Err(ApiError::bad_request(
            "Cannot specify both canary_device_labels and canary_device_ids.",
        ));
    }

    let res = if has_ids {
        let canary_device_ids = request
            .as_ref()
            .unwrap()
            .canary_device_ids
            .as_ref()
            .unwrap();
        sqlx::query!(
            r#"
            WITH selected_devices AS (
                SELECT d.id FROM device d
                JOIN release r ON d.release_id = r.id
                WHERE
                    d.id = ANY($3)
                    AND d.release_id = d.target_release_id
                    AND d.release_id <> $4
                    AND d.last_ping > NOW() - INTERVAL '3 minutes'
                    AND r.distribution_id = $1
            )
            INSERT INTO deployment_devices (deployment_id, device_id)
            SELECT $2, id FROM selected_devices
            "#,
            release.distribution_id,
            deployment.id,
            canary_device_ids.as_slice(),
            release_id
        )
        .execute(&mut *tx)
        .await?
    } else if has_labels {
        let canary_device_labels = request
            .as_ref()
            .unwrap()
            .canary_device_labels
            .as_ref()
            .unwrap();
        sqlx::query!(
            r#"
            WITH selected_devices AS (
                SELECT DISTINCT d.id FROM device d
                JOIN release r ON d.release_id = r.id
                LEFT JOIN device_label dl ON dl.device_id = d.id
                LEFT JOIN label l ON l.id = dl.label_id
                WHERE
                    l.name || '=' || dl.value = ANY($3)
                    AND d.last_ping > NOW() - INTERVAL '3 minutes'
                    AND d.release_id = d.target_release_id
                    AND d.release_id <> $4
                    AND r.distribution_id = $1
            )
            INSERT INTO deployment_devices (deployment_id, device_id)
            SELECT $2, id FROM selected_devices
            "#,
            release.distribution_id,
            deployment.id,
            canary_device_labels.as_slice(),
            release_id
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
                WHERE d.last_ping > NOW() - INTERVAL '3 minutes'
                AND d.release_id = d.target_release_id
                AND d.release_id <> $3
                AND d.follow_latest
                AND r.distribution_id = $1
                ORDER BY
                    -- Deprioritize devices with unhealthy watchdog services
                    CASE WHEN EXISTS (
                        SELECT 1 FROM device_service_status dss
                        JOIN release_services rs ON rs.id = dss.release_service_id
                        WHERE dss.device_id = d.id
                          AND rs.release_id = d.release_id
                          AND rs.watchdog_sec IS NOT NULL
                          AND dss.active_state != 'active'
                    ) THEN 1 ELSE 0 END ASC,
                    COALESCE(dn.network_score, 0) DESC,
                    d.last_ping DESC
                LIMIT 10
            )
            INSERT INTO deployment_devices (deployment_id, device_id)
            SELECT $2, id FROM selected_devices
            ",
            release.distribution_id,
            deployment.id,
            release_id
        )
        .execute(&mut *tx)
        .await?
    };
    let canary_count = res.rows_affected();
    if canary_count == 0 {
        tx.rollback().await?;
        return Err(ApiError::bad_request(
            "Canary release contains no devices, aborting.",
        ));
    }

    sqlx::query!(
        "
        UPDATE device
        SET target_release_id = $1, target_release_id_set_at = NOW()
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

    // Send Slack notification
    if let Some(deployment_slack_hook_url) = &config.deployment_slack_hook_url {
        let release_info = sqlx::query!(
            "SELECT r.version, d.name as distribution_name
             FROM release r
             JOIN distribution d ON r.distribution_id = d.id
             WHERE r.id = $1",
            release_id
        )
        .fetch_optional(pg_pool)
        .await;

        if let Ok(Some(info)) = release_info {
            let triggered_by = user_email.unwrap_or("Unknown");
            let message = json!({
                "blocks": [
                    {
                        "type": "section",
                        "text": {
                            "type": "mrkdwn",
                            "text": format!(
                                ":rocket: *Deployment Started*\n\n*Distribution:* {}\n*Version:* {}\n*Canary devices:* {}\n*Triggered by:* {}",
                                info.distribution_name,
                                info.version,
                                canary_count,
                                triggered_by
                            )
                        }
                    }
                ]
            });
            send_slack_notification(deployment_slack_hook_url, message).await;
        }
    }

    Ok(deployment)
}

pub async fn confirm_full_rollout(
    release_id: i32,
    pg_pool: &PgPool,
    config: &Config,
    user_email: Option<&str>,
) -> Result<Deployment, ApiError> {
    let mut tx = pg_pool.begin().await?;

    // Row lock so two confirms racing each other cannot both retarget the fleet.
    let deployment = sqlx::query_as!(
        Deployment,
        r#"SELECT id, release_id, status AS "status!: DeploymentStatus", updated_at, created_at
           FROM deployment WHERE release_id = $1 FOR UPDATE"#,
        release_id
    )
    .fetch_one(&mut *tx)
    .await?;

    match deployment.status {
        DeploymentStatus::Done => return Ok(deployment),
        DeploymentStatus::InProgress => {}
        DeploymentStatus::Failed | DeploymentStatus::Canceled => {
            return Err(ApiError::bad_request(format!(
                "Cannot confirm full rollout: deployment is {}",
                deployment.status
            )));
        }
    }

    let release = sqlx::query!(
        "SELECT distribution_id, release_candidate, yanked FROM release WHERE id = $1",
        release_id
    )
    .fetch_one(&mut *tx)
    .await?;

    if release.release_candidate {
        return Err(ApiError::bad_request(
            "Cannot perform full rollout for a release candidate",
        ));
    }
    if release.yanked {
        return Err(ApiError::bad_request(
            "Cannot perform full rollout for a yanked release",
        ));
    }

    let device_ids = sqlx::query_scalar!(
        "SELECT device_id FROM deployment_devices WHERE deployment_id = $1",
        deployment.id
    )
    .fetch_all(&mut *tx)
    .await?;

    if device_ids.is_empty() {
        return Err(ApiError::bad_request(
            "Cannot confirm full rollout: no canary devices found in deployment",
        ));
    }

    let mismatched_devices_count = sqlx::query_scalar!(
        "SELECT COUNT(*)
             FROM device
             WHERE id = ANY($1) AND release_id IS DISTINCT FROM target_release_id",
        &device_ids
    )
    .fetch_one(&mut *tx)
    .await?
    .unwrap_or(0);

    if mismatched_devices_count > 0 {
        return Err(ApiError::bad_request(format!(
            "Cannot confirm full rollout: {mismatched_devices_count} canary device(s) have not completed updating"
        )));
    }

    // Check service health for watchdog services
    let watchdog_service_count = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM release_services WHERE release_id = $1 AND watchdog_sec IS NOT NULL",
        release_id
    )
    .fetch_one(&mut *tx)
    .await?
    .unwrap_or(0);

    if watchdog_service_count > 0 {
        let unhealthy_device_count = sqlx::query_scalar!(
            "SELECT COUNT(DISTINCT d.id)
             FROM device d
             WHERE d.id = ANY($1)
             AND EXISTS (
                 SELECT 1 FROM release_services rs
                 WHERE rs.release_id = $2 AND rs.watchdog_sec IS NOT NULL
                 AND NOT EXISTS (
                     SELECT 1 FROM device_service_status dss
                     WHERE dss.device_id = d.id
                     AND dss.release_service_id = rs.id
                     AND dss.active_state = 'active'
                 )
             )",
            &device_ids,
            release_id
        )
        .fetch_one(&mut *tx)
        .await?
        .unwrap_or(0);

        if unhealthy_device_count > 0 {
            return Err(ApiError::bad_request(format!(
                "Cannot confirm full rollout: {unhealthy_device_count} canary device(s) have unhealthy or unreported services"
            )));
        }
    }

    let updated_deployment = sqlx::query_as!(
            Deployment,
            "UPDATE deployment SET status = 'done', updated_at = NOW()
             WHERE release_id = $1
             RETURNING id, release_id, status AS \"status!: DeploymentStatus\", updated_at, created_at",
            release_id
        )
        .fetch_one(&mut *tx)
        .await?;

    // Devices that do not follow latest stay where they are: a fleet-wide
    // rollout must never move a pinned device. The count reported below comes
    // from this statement so it can't drift from what was actually retargeted.
    let device_count = sqlx::query!(
        "UPDATE device
             SET target_release_id = $1, target_release_id_set_at = NOW()
             WHERE device.release_id IN (
                SELECT id FROM release WHERE distribution_id = $2
             )
             AND device.follow_latest",
        release_id,
        release.distribution_id
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    sqlx::query!(
        "UPDATE distribution SET latest_release_id = $1 WHERE id = $2",
        release_id,
        release.distribution_id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    // Send Slack notification
    if let Some(deployment_slack_hook_url) = &config.deployment_slack_hook_url {
        let release_info = sqlx::query!(
            "SELECT r.version, d.name as distribution_name
             FROM release r
             JOIN distribution d ON r.distribution_id = d.id
             WHERE r.id = $1",
            release_id
        )
        .fetch_optional(pg_pool)
        .await;

        if let Ok(Some(info)) = release_info {
            let confirmed_by = user_email.unwrap_or("Unknown");
            let message = json!({
                "blocks": [
                    {
                        "type": "section",
                        "text": {
                            "type": "mrkdwn",
                            "text": format!(
                                ":white_check_mark: *Full Rollout Confirmed*\n\n*Distribution:* {}\n*Version:* {}\n*Devices updated:* {}\n*Confirmed by:* {}",
                                info.distribution_name,
                                info.version,
                                device_count,
                                confirmed_by
                            )
                        }
                    }
                ]
            });
            send_slack_notification(deployment_slack_hook_url, message).await;
        }
    }

    Ok(updated_deployment)
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct RollbackRequest {
    /// Stable release in the same distribution to move devices to.
    pub target_release_id: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct RollbackResult {
    pub target_release_id: i32,
    pub devices_retargeted: i64,
}

/// Disaster recovery for a yanked release: every device targeting it is moved
/// to `target_release_id` at once, skipping the canary phase, because leaving
/// the fleet on a withdrawn release for a canary round is the worse outcome.
pub async fn rollback_release(
    release_id: i32,
    target_release_id: i32,
    pg_pool: &PgPool,
    config: &Config,
    user_email: Option<&str>,
) -> Result<RollbackResult, ApiError> {
    if release_id == target_release_id {
        return Err(ApiError::bad_request(
            "Cannot roll back a release onto itself.",
        ));
    }

    let mut tx = pg_pool.begin().await?;

    let source = sqlx::query!(
        "SELECT distribution_id, yanked, version FROM release WHERE id = $1 FOR UPDATE",
        release_id
    )
    .fetch_one(&mut *tx)
    .await?;

    // Without the yank, a later fleet rollout or the canary could send devices
    // straight back to the release we are rolling away from.
    if !source.yanked {
        return Err(ApiError::bad_request(
            "Only a yanked release can be rolled back.",
        ));
    }

    let target = sqlx::query!(
        "SELECT distribution_id, draft, yanked, release_candidate, version FROM release WHERE id = $1",
        target_release_id
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(target) = target else {
        return Err(ApiError::bad_request("Target release not found."));
    };
    if target.distribution_id != source.distribution_id {
        return Err(ApiError::bad_request(
            "Target release belongs to a different distribution.",
        ));
    }
    if target.draft || target.yanked || target.release_candidate {
        return Err(ApiError::bad_request(
            "Target release must be published, not yanked and not a release candidate.",
        ));
    }

    sqlx::query!(
        "UPDATE deployment SET status = 'canceled', updated_at = NOW()
         WHERE release_id = $1 AND status = 'in_progress'",
        release_id
    )
    .execute(&mut *tx)
    .await?;

    // Pinned devices move too: a yanked release cannot be pinned to anymore,
    // so a device still targeting it is exactly what this is meant to rescue.
    let devices_retargeted = sqlx::query!(
        "UPDATE device
         SET target_release_id = $2, target_release_id_set_at = NOW()
         WHERE target_release_id = $1",
        release_id,
        target_release_id
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    sqlx::query!(
        "UPDATE distribution SET latest_release_id = $2
         WHERE id = $3 AND latest_release_id = $1",
        release_id,
        target_release_id,
        source.distribution_id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    if let Some(deployment_slack_hook_url) = &config.deployment_slack_hook_url {
        let triggered_by = user_email.unwrap_or("Unknown");
        let message = json!({
            "blocks": [
                {
                    "type": "section",
                    "text": {
                        "type": "mrkdwn",
                        "text": format!(
                            ":rewind: *Rollback*\n\n*From:* {} (yanked)\n*To:* {}\n*Devices retargeted:* {}\n*Triggered by:* {}",
                            source.version,
                            target.version,
                            devices_retargeted,
                            triggered_by
                        )
                    }
                }
            ]
        });
        send_slack_notification(deployment_slack_hook_url, message).await;
    }

    Ok(RollbackResult {
        target_release_id,
        devices_retargeted: i64::try_from(devices_retargeted).unwrap_or(i64::MAX),
    })
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

    let devices = sqlx::query_as!(
        DeploymentDeviceWithStatus,
        r#"
            SELECT
                d.id AS device_id,
                d.serial_number,
                d.release_id,
                d.target_release_id,
                d.last_ping,
                dd.created_at AS added_at,
                COALESCE(JSONB_OBJECT_AGG(l.name, dl.value) FILTER (WHERE l.name IS NOT NULL), '{}') as "labels!: SqlxJson<HashMap<String, String>>"
            FROM deployment_devices dd
            JOIN device d ON dd.device_id = d.id
            LEFT JOIN device_label dl ON dl.device_id = d.id
            LEFT JOIN label l ON l.id = dl.label_id
            WHERE dd.deployment_id = $1
            GROUP BY d.id, d.serial_number, d.release_id, d.target_release_id, d.last_ping, dd.created_at
            ORDER BY dd.created_at ASC
            "#,
        deployment.id
    )
    .fetch_all(pg_pool)
    .await?;

    Ok(devices)
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct DeviceServiceHealth {
    pub device_id: i32,
    pub serial_number: String,
    pub release_service_id: i32,
    pub service_name: String,
    pub active_state: String,
    pub n_restarts: i32,
    pub checked_at: DateTime<Utc>,
}

pub async fn get_deployment_service_health(
    release_id: i32,
    pg_pool: &PgPool,
) -> anyhow::Result<Vec<DeviceServiceHealth>> {
    let deployment = sqlx::query!(
        "SELECT id FROM deployment WHERE release_id = $1",
        release_id
    )
    .fetch_optional(pg_pool)
    .await?;

    let Some(deployment) = deployment else {
        return Ok(Vec::new());
    };

    let health = sqlx::query_as!(
        DeviceServiceHealth,
        r#"
        SELECT
            d.id AS device_id,
            d.serial_number,
            dss.release_service_id,
            rs.service_name,
            dss.active_state,
            dss.n_restarts,
            dss.checked_at
        FROM deployment_devices dd
        JOIN device d ON dd.device_id = d.id
        JOIN device_service_status dss ON dss.device_id = d.id
        JOIN release_services rs ON rs.id = dss.release_service_id
        WHERE dd.deployment_id = $1
          AND rs.release_id = $2
          AND rs.watchdog_sec IS NOT NULL
        ORDER BY d.serial_number, rs.service_name
        "#,
        deployment.id,
        release_id
    )
    .fetch_all(pg_pool)
    .await?;

    Ok(health)
}
