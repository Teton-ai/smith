use std::cmp::Reverse;
use std::collections::{HashMap, VecDeque};

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

/// Most buckets a staggered retarget is split into; smaller sets get one device
/// per bucket.
pub const STAGGER_MAX_BUCKETS: usize = 20;
pub const STAGGER_INTERVAL_MINUTES: i32 = 5;

/// Deploy to Selected above this many devices is staggered unless the caller
/// opts out.
pub const BULK_STAGGER_THRESHOLD: usize = 15;

pub fn bucket_count(device_count: usize) -> usize {
    device_count.min(STAGGER_MAX_BUCKETS)
}

/// Consecutive chunks of the ranked devices rather than a modulo, so bucket
/// sizes differ by at most one device whatever the set size.
pub fn bucket_for_rank(rank: usize, device_count: usize) -> usize {
    if device_count == 0 {
        return 0;
    }
    rank * bucket_count(device_count) / device_count
}

/// Minutes between the first and the last bucket opening.
pub fn stagger_window_minutes(device_count: usize) -> i32 {
    bucket_count(device_count).saturating_sub(1) as i32 * STAGGER_INTERVAL_MINUTES
}

/// Deploy to Selected: the stagger window for `device_count` distinct devices,
/// or `None` when they all move at once.
pub fn bulk_stagger_window(device_count: usize, all_at_once: bool) -> Option<i32> {
    (device_count > BULK_STAGGER_THRESHOLD && !all_at_once)
        .then(|| stagger_window_minutes(device_count))
}

#[derive(PartialEq, Eq, Hash)]
enum Network {
    Ip(i64),
    /// A device without an IP is a network of its own.
    Alone(i32),
}

/// Each pick takes the next device (input = tie-break order) of the network with most left; a network
/// that took `ceil(left / groups left)` in the current group waits while others can fill it.
pub fn spread_by_network(
    devices: impl IntoIterator<Item = (i32, Option<i64>)>,
    group_of: impl Fn(usize) -> usize,
) -> Vec<i32> {
    let mut by_network: HashMap<Network, VecDeque<(usize, i32)>> = HashMap::new();
    let mut device_count = 0usize;
    for (position, (id, ip)) in devices.into_iter().enumerate() {
        by_network
            .entry(ip.map_or(Network::Alone(id), Network::Ip))
            .or_default()
            .push_back((position, id));
        device_count += 1;
    }
    let group_count = device_count
        .checked_sub(1)
        .map_or(0, |last| group_of(last) + 1);
    // (devices left, quota in the current group, picked in the current group)
    let mut networks: Vec<_> = by_network.into_values().map(|left| (left, 0, 0)).collect();

    let mut order = Vec::with_capacity(device_count);
    let mut current_group = None;
    loop {
        let group = group_of(order.len());
        if current_group != Some(group) {
            current_group = Some(group);
            let groups_left = group_count.saturating_sub(group).max(1);
            for (left, quota, picked) in &mut networks {
                *quota = left.len().div_ceil(groups_left);
                *picked = 0;
            }
        }
        let Some((left, _, picked)) = networks
            .iter_mut()
            .filter(|(left, _, _)| !left.is_empty())
            .min_by_key(|(left, quota, picked)| {
                (
                    picked >= quota,
                    Reverse(left.len()),
                    left.front().map(|d| d.0),
                )
            })
        else {
            return order;
        };
        if let Some((_, id)) = left.pop_front() {
            order.push(id);
            *picked += 1;
        }
    }
}

pub struct SelectedRetarget {
    /// Distinct devices retargeted.
    pub device_count: usize,
    /// `None` when every device moved at once.
    pub window_minutes: Option<i32>,
}

/// Deploy to Selected: staggered above the bulk threshold unless the caller
/// opts out, immediate otherwise.
pub async fn retarget_selected(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    release_id: i32,
    device_ids: &[i32],
    all_at_once: bool,
) -> sqlx::Result<SelectedRetarget> {
    // Duplicates would push a small selection over the threshold.
    let mut unique_ids = device_ids.to_vec();
    unique_ids.sort_unstable();
    unique_ids.dedup();

    if bulk_stagger_window(unique_ids.len(), all_at_once).is_some() {
        let staggered = stagger_retarget(tx, release_id, &unique_ids).await?;
        return Ok(SelectedRetarget {
            device_count: unique_ids.len(),
            window_minutes: Some(staggered.window_minutes),
        });
    }

    sqlx::query!(
        "UPDATE device SET target_release_id = $1, target_release_id_set_at = NOW(), target_release_eligible_at = NULL WHERE id = ANY($2)",
        release_id,
        &unique_ids
    )
    .execute(&mut **tx)
    .await?;
    Ok(SelectedRetarget {
        device_count: unique_ids.len(),
        window_minutes: None,
    })
}

pub struct StaggerOutcome {
    /// Every device now targeting the release, including those already on it.
    pub retargeted: u64,
    /// Minutes until the last bucket opens.
    pub window_minutes: i32,
}

/// Points `device_ids` at `release_id`, letting each device move only once its
/// bucket opens.
pub async fn stagger_retarget(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    release_id: i32,
    device_ids: &[i32],
) -> sqlx::Result<StaggerOutcome> {
    // Devices already on the release (canaries) download nothing, so they take
    // no bucket slot and keep the buckets even for the devices that do.
    let already_on_release = sqlx::query!(
        "UPDATE device
         SET target_release_id = $1,
             target_release_id_set_at = NOW(),
             target_release_eligible_at = NULL
         WHERE id = ANY($2) AND release_id = $1",
        release_id,
        device_ids
    )
    .execute(&mut **tx)
    .await?
    .rows_affected();

    // Ranked by a hash of device and release, not by id: sites are provisioned
    // in contiguous ids, and who goes first should change with every release.
    // md5 in Postgres is stable across API restarts and replicas.
    let devices = sqlx::query!(
        "SELECT id, ip_address_id FROM device
         WHERE id = ANY($1) AND release_id IS DISTINCT FROM $2
         ORDER BY md5(id::text || ':' || $2::int::text), id",
        device_ids,
        release_id
    )
    .fetch_all(&mut **tx)
    .await?;

    let device_count = devices.len();
    let ranked_ids = spread_by_network(devices.iter().map(|d| (d.id, d.ip_address_id)), |rank| {
        bucket_for_rank(rank, device_count)
    });
    let offset_minutes: Vec<i32> = (0..device_count)
        .map(|rank| bucket_for_rank(rank, device_count) as i32 * STAGGER_INTERVAL_MINUTES)
        .collect();

    // Eligibility uses the database clock, the same one the poll compares it to.
    let staggered = sqlx::query!(
        "UPDATE device d
         SET target_release_id = $1,
             target_release_id_set_at = NOW(),
             target_release_eligible_at = NOW() + make_interval(mins => u.offset_minutes)
         FROM UNNEST($2::int[], $3::int[]) AS u(id, offset_minutes)
         WHERE d.id = u.id",
        release_id,
        &ranked_ids,
        &offset_minutes
    )
    .execute(&mut **tx)
    .await?
    .rows_affected();

    Ok(StaggerOutcome {
        retargeted: already_on_release + staggered,
        window_minutes: stagger_window_minutes(device_count),
    })
}

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
        let eligible = sqlx::query!(
            r#"
            SELECT d.id, d.ip_address_id,
                -- Unhealthy watchdog services only fill canary slots healthy devices leave
                NOT EXISTS (
                    SELECT 1 FROM device_service_status dss
                    JOIN release_services rs ON rs.id = dss.release_service_id
                    WHERE dss.device_id = d.id
                      AND rs.release_id = d.release_id
                      AND rs.watchdog_sec IS NOT NULL
                      AND dss.active_state != 'active'
                ) AS "healthy!"
            FROM device d
            JOIN release r ON d.release_id = r.id
            LEFT JOIN device_network dn ON d.id = dn.device_id
            WHERE d.last_ping > NOW() - INTERVAL '3 minutes'
            AND d.release_id = d.target_release_id
            AND d.release_id <> $2
            AND d.follow_latest
            AND r.distribution_id = $1
            ORDER BY
                COALESCE(dn.network_score, 0) DESC,
                d.last_ping DESC
            "#,
            release.distribution_id,
            release_id
        )
        .fetch_all(&mut *tx)
        .await?;
        const CANARY_COUNT: usize = 10;
        let (healthy, unhealthy): (Vec<_>, Vec<_>) = eligible.iter().partition(|d| d.healthy);
        let selected: Vec<i32> = [healthy, unhealthy]
            .into_iter()
            .flat_map(|devices| {
                // One group of CANARY_COUNT, then one-device groups, so every network's quota is 1.
                spread_by_network(
                    devices.into_iter().map(|d| (d.id, d.ip_address_id)),
                    |rank| if rank < CANARY_COUNT { 0 } else { rank },
                )
            })
            .take(CANARY_COUNT)
            .collect();
        sqlx::query!(
            "INSERT INTO deployment_devices (deployment_id, device_id) SELECT $1, UNNEST($2::int[])",
            deployment.id,
            &selected
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
        SET target_release_id = $1, target_release_id_set_at = NOW(), target_release_eligible_at = NULL
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
    // rollout must never move a pinned device. The rows stay locked until the
    // retarget so a concurrent pin is either seen here or waits for us, and the
    // count reported below comes from the retarget itself.
    let follower_ids = sqlx::query_scalar!(
        "SELECT id FROM device
             WHERE device.release_id IN (
                SELECT id FROM release WHERE distribution_id = $1
             )
             AND device.follow_latest
             FOR UPDATE",
        release.distribution_id
    )
    .fetch_all(&mut *tx)
    .await?;

    let device_count = stagger_retarget(&mut tx, release_id, &follower_ids)
        .await?
        .retargeted;

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
         SET target_release_id = $2, target_release_id_set_at = NOW(), target_release_eligible_at = NULL
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

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct StaggerPolicy {
    /// Deploy to Selected staggers selections of more than this many devices.
    pub bulk_threshold: i32,
    /// Most buckets a staggered rollout is split into.
    pub max_buckets: i32,
    /// Minutes between two buckets opening.
    pub interval_minutes: i32,
    /// Whether Deploy to Selected staggers the requested number of devices.
    pub staggered: bool,
    /// Minutes until the last bucket opens for that many devices; 0 when not
    /// staggered.
    pub window_minutes: i32,
}

pub fn stagger_policy(device_count: usize) -> StaggerPolicy {
    let window = bulk_stagger_window(device_count, false);
    StaggerPolicy {
        bulk_threshold: BULK_STAGGER_THRESHOLD as i32,
        max_buckets: STAGGER_MAX_BUCKETS as i32,
        interval_minutes: STAGGER_INTERVAL_MINUTES,
        staggered: window.is_some(),
        window_minutes: window.unwrap_or(0),
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct FinishNowResult {
    pub devices_released: i64,
}

/// Opens every remaining bucket of the release at once.
pub async fn release_waiting_devices<'e, E>(release_id: i32, executor: E) -> sqlx::Result<u64>
where
    E: sqlx::PgExecutor<'e>,
{
    Ok(sqlx::query!(
        "UPDATE device SET target_release_eligible_at = NOW()
         WHERE target_release_id = $1
         AND target_release_eligible_at > NOW()
         AND release_id IS DISTINCT FROM target_release_id
         AND archived = false",
        release_id
    )
    .execute(executor)
    .await?
    .rows_affected())
}

pub async fn finish_now(
    release_id: i32,
    pg_pool: &PgPool,
    config: &Config,
    user_email: Option<&str>,
) -> Result<FinishNowResult, ApiError> {
    let devices_released = release_waiting_devices(release_id, pg_pool).await?;

    if devices_released > 0
        && let Some(deployment_slack_hook_url) = &config.deployment_slack_hook_url
    {
        let release_info = sqlx::query!(
            "SELECT r.version, d.name as distribution_name
             FROM release r
             JOIN distribution d ON r.distribution_id = d.id
             WHERE r.id = $1",
            release_id
        )
        .fetch_optional(pg_pool)
        .await;

        match release_info {
            Ok(Some(info)) => {
                let triggered_by = user_email.unwrap_or("Unknown");
                let message = json!({
                    "blocks": [
                        {
                            "type": "section",
                            "text": {
                                "type": "mrkdwn",
                                "text": format!(
                                    ":fast_forward: *Staggered Rollout Finished Early*\n\n*Distribution:* {}\n*Version:* {}\n*Devices released at once:* {}\n*Triggered by:* {}",
                                    info.distribution_name,
                                    info.version,
                                    devices_released,
                                    triggered_by
                                )
                            }
                        }
                    ]
                });
                send_slack_notification(deployment_slack_hook_url, message).await;
            }
            Ok(None) => {}
            Err(err) => tracing::error!("Failed to look up release {release_id} for Slack: {err}"),
        }
    }

    Ok(FinishNowResult {
        devices_released: i64::try_from(devices_released).unwrap_or(i64::MAX),
    })
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct RolloutProgress {
    /// Not on the release yet and waiting for their bucket to open.
    pub scheduled: i64,
    /// Allowed to move but not on the release yet.
    pub eligible: i64,
    /// Already on the release.
    pub updated: i64,
    pub next_bucket_at: Option<chrono::DateTime<chrono::Utc>>,
    /// When the last bucket opens (or opened).
    pub expected_end_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Counts every device currently targeting the release, so a device moved to
/// another release afterwards drops out.
pub async fn rollout_progress<'e, E>(release_id: i32, executor: E) -> sqlx::Result<RolloutProgress>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query_as!(
        RolloutProgress,
        r#"SELECT
            COUNT(*) FILTER (
                WHERE release_id IS DISTINCT FROM target_release_id
                AND target_release_eligible_at > NOW()
            ) AS "scheduled!",
            COUNT(*) FILTER (
                WHERE release_id IS DISTINCT FROM target_release_id
                AND (target_release_eligible_at IS NULL OR target_release_eligible_at <= NOW())
            ) AS "eligible!",
            COUNT(*) FILTER (WHERE release_id = target_release_id) AS "updated!",
            MIN(target_release_eligible_at) FILTER (
                WHERE release_id IS DISTINCT FROM target_release_id
                AND target_release_eligible_at > NOW()
            ) AS next_bucket_at,
            MAX(target_release_eligible_at) AS expected_end_at
        FROM device
        WHERE target_release_id = $1 AND archived = false"#,
        release_id
    )
    .fetch_one(executor)
    .await
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn bucket_sizes(device_count: usize) -> Vec<usize> {
        let mut sizes = vec![0; bucket_count(device_count)];
        for rank in 0..device_count {
            sizes[bucket_for_rank(rank, device_count)] += 1;
        }
        sizes
    }

    #[test]
    fn bucket_sizes_differ_by_at_most_one() {
        for device_count in (1..=200).chain([5000]) {
            let sizes = bucket_sizes(device_count);
            let min = sizes.iter().min().copied().unwrap_or(0);
            let max = sizes.iter().max().copied().unwrap_or(0);
            assert!(max - min <= 1, "{device_count} devices: sizes {sizes:?}");
            assert_eq!(sizes.iter().sum::<usize>(), device_count);
        }
    }

    #[test]
    fn window_covers_every_bucket_but_the_first() {
        assert_eq!(stagger_window_minutes(0), 0);
        assert_eq!(stagger_window_minutes(1), 0);
        assert_eq!(stagger_window_minutes(16), 75);
        assert_eq!(stagger_window_minutes(5000), 95);
    }

    #[test]
    fn bulk_staggers_only_above_the_threshold_without_opt_out() {
        assert!(bulk_stagger_window(BULK_STAGGER_THRESHOLD, false).is_none());
        assert!(bulk_stagger_window(BULK_STAGGER_THRESHOLD + 1, false).is_some());
        assert!(bulk_stagger_window(BULK_STAGGER_THRESHOLD + 1, true).is_none());
    }

    #[test]
    fn bucket_count_is_capped() {
        assert_eq!(bucket_count(0), 0);
        assert_eq!(bucket_count(16), 16);
        assert_eq!(bucket_count(21), STAGGER_MAX_BUCKETS);
        assert_eq!(bucket_count(5000), STAGGER_MAX_BUCKETS);
    }

    #[test]
    fn spread_by_network_drains_biggest_networks_within_group_quota() {
        // Letters name networks; E has no IP, so each E device is a network of its own.
        let five_networks = [
            ('A', 9, Some(1)),
            ('B', 9, Some(2)),
            ('C', 3, Some(3)),
            ('D', 2, Some(4)),
            ('E', 2, None),
        ];
        let big_site = [('A', 10, Some(1)), ('E', 6, None)];
        let canary = |rank: usize, _| if rank < 10 { 0 } else { rank };
        let groups_of_four = |rank: usize, _| rank / 4;
        let cases = [
            (
                "stagger",
                &five_networks[..],
                bucket_for_rank as fn(usize, usize) -> usize,
                "AB|A|B|A|BA|B|A|B|AB|A|B|C|AB|C|D|A|BC|D|E|E",
            ),
            (
                "canary",
                &five_networks,
                canary,
                "ABCDEEABAB|A|B|A|B|A|B|A|B|A|B|C|A|B|C|D",
            ),
            (
                "big site never piles up at the tail",
                &big_site,
                groups_of_four,
                "AAAE|AAAE|AAEE|AAEE",
            ),
        ];
        for (name, networks, group_of, want) in cases {
            let devices: Vec<(i32, Option<i64>, char)> = networks
                .iter()
                .flat_map(|&(name, count, ip)| std::iter::repeat_n((ip, name), count))
                .zip(1..)
                .map(|((ip, name), id)| (id, ip, name))
                .collect();
            let group_of = |rank| group_of(rank, devices.len());
            let order = spread_by_network(devices.iter().map(|&(id, ip, _)| (id, ip)), group_of);
            let mut got = String::new();
            for (rank, id) in order.iter().enumerate() {
                if rank > 0 && group_of(rank) != group_of(rank - 1) {
                    got.push('|');
                }
                got.extend(devices.iter().find(|d| d.0 == *id).map(|d| d.2));
            }
            assert_eq!(got, want, "{name}");
        }
    }

    /// `N` releases of one distribution, so a test can move devices between them.
    pub(crate) async fn insert_releases<const N: usize>(pool: &PgPool) -> anyhow::Result<[i32; N]> {
        let distribution_id: i32 = sqlx::query_scalar(
            "INSERT INTO distribution (name) VALUES ('stagger-test') RETURNING id",
        )
        .fetch_one(pool)
        .await?;
        let ids: Vec<i32> = sqlx::query_scalar(
            "INSERT INTO release (distribution_id, version)
             SELECT $1, n || '.0.0' FROM generate_series(1, $2) AS n RETURNING id",
        )
        .bind(distribution_id)
        .bind(i32::try_from(N)?)
        .fetch_all(pool)
        .await?;
        ids.try_into()
            .map_err(|ids| anyhow::anyhow!("expected {N} releases, got {ids:?}"))
    }

    /// Devices on `release_id`, or with no release reported when `None`.
    pub(crate) async fn insert_devices(
        pool: &PgPool,
        count: usize,
        release_id: Option<i32>,
    ) -> anyhow::Result<Vec<i32>> {
        Ok(sqlx::query_scalar(
            "INSERT INTO device (serial_number, release_id)
             SELECT gen_random_uuid()::text, $2 FROM generate_series(1, $1)
             RETURNING id",
        )
        .bind(i32::try_from(count)?)
        .bind(release_id)
        .fetch_all(pool)
        .await?)
    }

    /// Minutes from now until each device's bucket opens; `None` for no bucket.
    async fn offsets_by_device(
        pool: &PgPool,
        device_ids: &[i32],
    ) -> anyhow::Result<HashMap<i32, Option<i64>>> {
        let rows: Vec<(i32, Option<f64>)> = sqlx::query_as(
            "SELECT id, EXTRACT(EPOCH FROM target_release_eligible_at - NOW())::float8
             FROM device WHERE id = ANY($1)",
        )
        .bind(device_ids)
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, secs)| (id, secs.map(|secs| (secs / 60.0).round() as i64)))
            .collect())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn stagger_retarget_spreads_movers_and_skips_canaries(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let [current, target, next] = insert_releases(&pool).await?;
        let movers = insert_devices(&pool, 19, Some(current)).await?;
        let canary = insert_devices(&pool, 1, Some(target)).await?;
        let all: Vec<i32> = movers.iter().chain(&canary).copied().collect();

        let mut tx = pool.begin().await?;
        let outcome = stagger_retarget(&mut tx, target, &all).await?;
        tx.commit().await?;

        assert_eq!(outcome.retargeted, 20);
        assert_eq!(
            outcome.window_minutes, 90,
            "the canary downloads nothing, so it doesn't widen the window"
        );
        let to_target = offsets_by_device(&pool, &all).await?;
        assert_eq!(to_target.get(&canary[0]), Some(&None));
        let mut minutes: Vec<i64> = movers
            .iter()
            .filter_map(|id| to_target.get(id).copied().flatten())
            .collect();
        minutes.sort_unstable();
        let expected: Vec<i64> = (0..19).map(|bucket| bucket * 5).collect();
        assert_eq!(minutes, expected, "one mover per bucket, 5 min apart");

        let mut tx = pool.begin().await?;
        stagger_retarget(&mut tx, next, &movers).await?;
        tx.commit().await?;
        let to_next = offsets_by_device(&pool, &movers).await?;
        let movers_to_target: HashMap<i32, Option<i64>> = movers
            .iter()
            .map(|id| (*id, to_target.get(id).copied().flatten()))
            .collect();
        assert_ne!(
            to_next, movers_to_target,
            "who goes first should change with the release"
        );
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn deploy_to_selected_counts_each_device_once(pool: PgPool) -> anyhow::Result<()> {
        let [target] = insert_releases(&pool).await?;
        let ids = insert_devices(&pool, BULK_STAGGER_THRESHOLD, None).await?;
        let doubled: Vec<i32> = ids.iter().chain(&ids).copied().collect();

        let mut tx = pool.begin().await?;
        let retarget = retarget_selected(&mut tx, target, &doubled, false).await?;
        tx.commit().await?;

        assert_eq!(retarget.device_count, BULK_STAGGER_THRESHOLD);
        assert_eq!(retarget.window_minutes, None);
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn progress_and_finish_now_follow_the_buckets(pool: PgPool) -> anyhow::Result<()> {
        let [current, target] = insert_releases(&pool).await?;
        let ids = insert_devices(&pool, 20, Some(current)).await?;
        let mut tx = pool.begin().await?;
        stagger_retarget(&mut tx, target, &ids).await?;
        tx.commit().await?;
        // Updated before its bucket opened: counted as updated, and finish now
        // must leave it alone.
        sqlx::query("UPDATE device SET release_id = target_release_id WHERE id = $1")
            .bind(ids[0])
            .execute(&pool)
            .await?;

        let progress = rollout_progress(target, &pool).await?;
        assert_eq!(progress.updated, 1);
        assert_eq!(progress.scheduled + progress.eligible, 19);
        assert!(
            progress.scheduled >= 18,
            "only bucket 0 is open: {progress:?}"
        );
        assert!(progress.next_bucket_at.is_some());
        assert!(progress.expected_end_at.is_some());

        let released = release_waiting_devices(target, &pool).await?;
        assert_eq!(i64::try_from(released)?, progress.scheduled);

        let after = rollout_progress(target, &pool).await?;
        assert_eq!(after.scheduled, 0);
        assert_eq!(after.eligible, 19);
        assert_eq!(after.next_bucket_at, None);
        Ok(())
    }
}
