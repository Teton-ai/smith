use std::cmp::Reverse;
use std::collections::{HashMap, VecDeque};

use chrono::{DateTime, Utc};
use models::deployment::Deployment;
use models::deployment::DeploymentRequest;
use models::deployment::DeploymentStatus;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::types::Json as SqlxJson;
use sqlx::types::chrono;
use sqlx::{PgConnection, PgPool};
use utoipa::ToSchema;

use crate::config::Config;
use crate::error::ApiError;
use crate::slack::send_slack_notification;
use crate::user::CurrentUser;

pub mod policy;
pub mod route;

const CANARY_COUNT: usize = 10;

const NO_CANARIES: &str = "No eligible online device can be a canary.";

/// Up to `CANARY_COUNT` canaries ranked by health and network spread: from `group`
/// when given, otherwise from devices following latest that aren't on a newer
/// release candidate.
async fn ranked_canaries(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    distribution_id: i32,
    release_id: i32,
) -> sqlx::Result<Vec<i32>> {
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
        AND r.distribution_id = $1
        AND d.follow_latest
        ORDER BY
            COALESCE(dn.network_score, 0) DESC,
            d.last_ping DESC
        "#,
        distribution_id,
        release_id
    )
    .fetch_all(&mut **tx)
    .await?;
    let (healthy, unhealthy): (Vec<_>, Vec<_>) = eligible.iter().partition(|d| d.healthy);
    Ok([healthy, unhealthy]
        .into_iter()
        .flat_map(|devices| {
            // One group of CANARY_COUNT, then one-device groups, so every network's quota is 1.
            spread_by_network(
                devices.into_iter().map(|d| (d.id, d.ip_address_id)),
                |rank| if rank < CANARY_COUNT { 0 } else { rank },
            )
        })
        .take(CANARY_COUNT)
        .collect())
}

async fn legacy_canaries(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    distribution_id: i32,
    release_id: i32,
    request: Option<&DeploymentRequest>,
) -> Result<Vec<i32>, ApiError> {
    let labels = request.and_then(|r| r.canary_device_labels.as_deref());
    let ids = request.and_then(|r| r.canary_device_ids.as_deref());
    if labels.is_some_and(|values| values.is_empty()) || ids.is_some_and(|values| values.is_empty())
    {
        return Err(ApiError::bad_request(
            "Canary labels and device IDs cannot be empty.",
        ));
    }
    if labels.is_some() && ids.is_some() {
        return Err(ApiError::bad_request(
            "Choose canaries by labels or by devices, not both.",
        ));
    }
    if labels.is_none() && ids.is_none() {
        return Ok(ranked_canaries(tx, distribution_id, release_id).await?);
    }
    Ok(sqlx::query_scalar!(
        "SELECT d.id FROM device d
         JOIN release r ON d.release_id = r.id
         WHERE r.distribution_id = $1 AND d.release_id <> $2
           AND d.release_id = d.target_release_id
           AND d.last_ping > NOW() - INTERVAL '3 minutes'
           AND (($3::int[] IS NOT NULL AND d.id = ANY($3)) OR
                ($4::text[] IS NOT NULL AND EXISTS (
                    SELECT 1 FROM device_label dl JOIN label l ON l.id = dl.label_id
                    WHERE dl.device_id = d.id AND l.name || '=' || dl.value = ANY($4)
                )))",
        distribution_id,
        release_id,
        ids,
        labels
    )
    .fetch_all(&mut **tx)
    .await?)
}

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

/// Which wave each device goes in. Devices already on the release aren't in it.
#[derive(Debug, Default)]
pub struct RolloutPlan {
    pub device_ids: Vec<i32>,
    pub waves: Vec<i32>,
}

pub struct SelectedRetarget {
    /// Distinct devices retargeted.
    pub device_count: usize,
    /// `None` when every device moved at once.
    pub window_minutes: Option<i32>,
    pub plan: RolloutPlan,
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
            plan: staggered.plan,
        });
    }

    let movers = sqlx::query_scalar!(
        "SELECT id FROM device WHERE id = ANY($1) AND release_id IS DISTINCT FROM $2",
        &unique_ids,
        release_id
    )
    .fetch_all(&mut **tx)
    .await?;
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
        plan: RolloutPlan {
            waves: vec![1; movers.len()],
            device_ids: movers,
        },
    })
}

/// Records a Deploy to Selected as a deployment, so it can be followed and
/// controlled like any other. `None` when no device needs to move.
pub async fn record_selected_deployment(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    release_id: i32,
    retarget: &SelectedRetarget,
    user_id: i32,
) -> sqlx::Result<Option<i32>> {
    if retarget.plan.device_ids.is_empty() {
        return Ok(None);
    }
    let deployment_id = sqlx::query_scalar!(
        "INSERT INTO deployment
            (release_id, status, kind, base_release_id, created_by, promoted_at, promoted_by,
             times_out_at)
         SELECT $1, 'done', 'direct', d.latest_release_id, $2, NOW(), $2,
             NOW() + make_interval(mins => $3)
         FROM release r JOIN distribution d ON d.id = r.distribution_id
         WHERE r.id = $1
         RETURNING id",
        release_id,
        user_id,
        timeout_minutes(retarget.window_minutes.unwrap_or(0))
    )
    .fetch_one(&mut **tx)
    .await?;
    record_plan(tx, deployment_id, &retarget.plan).await?;

    let count = retarget.plan.device_ids.len();
    let detail = match retarget.window_minutes {
        Some(minutes) => format!("{count} selected devices, staggered over {minutes} minutes"),
        None => format!("{count} selected devices, all at once"),
    };
    record_event(tx, deployment_id, "started", Some(user_id), Some(&detail)).await?;
    Ok(Some(deployment_id))
}

pub struct StaggerOutcome {
    /// Every device now targeting the release, including those already on it.
    pub retargeted: u64,
    /// Minutes until the last bucket opens.
    pub window_minutes: i32,
    pub plan: RolloutPlan,
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
        plan: RolloutPlan {
            waves: (0..device_count)
                .map(|rank| bucket_for_rank(rank, device_count) as i32 + 1)
                .collect(),
            device_ids: ranked_ids,
        },
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

/// The most recent deployment of a release.
pub async fn get_deployment(
    release_id: i32,
    pg_pool: &PgPool,
) -> anyhow::Result<Option<Deployment>> {
    Ok(sqlx::query_as!(
        Deployment,
        r#"
            SELECT id, release_id, status AS "status!: DeploymentStatus", updated_at, created_at
            FROM deployment WHERE release_id = $1
            ORDER BY id DESC LIMIT 1
            "#,
        release_id
    )
    .fetch_optional(pg_pool)
    .await?)
}

async fn latest_deployment_id(release_id: i32, pg_pool: &PgPool) -> Result<i32, ApiError> {
    sqlx::query_scalar!(
        "SELECT id FROM deployment WHERE release_id = $1 ORDER BY id DESC LIMIT 1",
        release_id
    )
    .fetch_optional(pg_pool)
    .await?
    .ok_or(ApiError::NotFound)
}

async fn record_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    deployment_id: i32,
    kind: &str,
    user_id: Option<i32>,
    detail: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT INTO deployment_event (deployment_id, kind, user_id, detail) VALUES ($1, $2, $3, $4)",
        deployment_id,
        kind,
        user_id,
        detail
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Adds the devices a rollout just scheduled to the deployment's plan.
async fn record_plan(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    deployment_id: i32,
    plan: &RolloutPlan,
) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT INTO deployment_devices (deployment_id, device_id, wave, from_release_id, opens_at)
         SELECT $1, d.id, u.wave, d.release_id, COALESCE(d.target_release_eligible_at, NOW())
         FROM UNNEST($2::int[], $3::int[]) AS u(id, wave)
         JOIN device d ON d.id = u.id
         ON CONFLICT (deployment_id, device_id) DO UPDATE
            SET wave = EXCLUDED.wave, from_release_id = EXCLUDED.from_release_id, opens_at = EXCLUDED.opens_at",
        deployment_id,
        &plan.device_ids,
        &plan.waves
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn release_label(release_id: i32, pg_pool: &PgPool) -> Option<(String, String)> {
    sqlx::query!(
        "SELECT r.version, d.name AS distribution_name
         FROM release r JOIN distribution d ON r.distribution_id = d.id
         WHERE r.id = $1",
        release_id
    )
    .fetch_optional(pg_pool)
    .await
    .inspect_err(|e| tracing::error!("Failed to look up release {release_id} for Slack: {e}"))
    .ok()
    .flatten()
    .map(|row| (row.distribution_name, row.version))
}

/// Posts to the deployment channel, if one is configured.
async fn notify(config: &Config, text: String) {
    if let Some(hook_url) = &config.deployment_slack_hook_url {
        let message = json!({
            "blocks": [{ "type": "section", "text": { "type": "mrkdwn", "text": text } }]
        });
        send_slack_notification(hook_url, message).await;
    }
}

/// A fleet deployment in this distribution that has yet to settle. Fleet rollouts
/// run one at a time, because the later one to confirm would otherwise undo the
/// earlier one. Release candidates roll out to pilot groups and never become what
/// the fleet follows, so they don't count.
async fn active_fleet_deployment<'e, E>(
    distribution_id: i32,
    excluding: Option<i32>,
    executor: E,
) -> sqlx::Result<Option<ActiveDeployment>>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query_as!(
        ActiveDeployment,
        r#"SELECT d.id, r.version AS release_version
           FROM deployment d
           JOIN release r ON r.id = d.release_id
           WHERE r.distribution_id = $1
             AND d.id IS DISTINCT FROM $2
             AND d.kind = 'staged'
             AND NOT r.release_candidate
             AND d.status NOT IN ('canceled', 'failed')
             AND (
                d.promoted_at IS NULL
                OR EXISTS (
                    SELECT 1 FROM deployment_device_state s
                    WHERE s.deployment_id = d.id AND s.state IN ('scheduled', 'updating')
                )
             )
           ORDER BY d.id DESC LIMIT 1"#,
        distribution_id,
        excluding
    )
    .fetch_optional(executor)
    .await
}

fn conflict_message(active: &ActiveDeployment) -> String {
    format!(
        "Deployment #{} of v{} is still active in this distribution. Wait for it to finish or cancel it first.",
        active.id, active.release_version
    )
}

pub async fn new_deployment(
    release_id: i32,
    request: Option<DeploymentRequest>,
    pg_pool: &PgPool,
    config: &Config,
    user: &CurrentUser,
    user_email: Option<&str>,
) -> Result<Deployment, ApiError> {
    policy::ensure_can_ship(user)?;
    let mut tx = pg_pool.begin().await?;
    let release = sqlx::query!(
        "SELECT distribution_id, draft, yanked, release_candidate FROM release WHERE id = $1",
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

    // Serializes starts within a distribution so two can't both pass the check below.
    let base_release_id = sqlx::query_scalar!(
        "SELECT latest_release_id FROM distribution WHERE id = $1 FOR UPDATE",
        release.distribution_id
    )
    .fetch_one(&mut *tx)
    .await?;

    if !release.release_candidate
        && let Some(active) =
            active_fleet_deployment(release.distribution_id, None, &mut *tx).await?
    {
        return Err(ApiError::bad_request(conflict_message(&active)));
    }

    let deployment = sqlx::query_as!(
        Deployment,
        r#"
        INSERT INTO deployment (release_id, status, base_release_id, created_by)
        VALUES ($1, 'in_progress', $2, $3)
        ON CONFLICT (release_id) WHERE status = 'in_progress' DO NOTHING
        RETURNING id, release_id, status AS "status!: DeploymentStatus", updated_at, created_at
        "#,
        release_id,
        base_release_id,
        user.user_id
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(deployment) = deployment else {
        return Err(ApiError::bad_request(
            "A deployment for this release is already in progress.",
        ));
    };

    let canary_ids = legacy_canaries(
        &mut tx,
        release.distribution_id,
        release_id,
        request.as_ref(),
    )
    .await?;
    if canary_ids.is_empty() {
        return Err(ApiError::bad_request(NO_CANARIES));
    }

    let plan = RolloutPlan {
        waves: vec![0; canary_ids.len()],
        device_ids: canary_ids,
    };
    sqlx::query!(
        "UPDATE device
         SET target_release_id = $1, target_release_id_set_at = NOW(), target_release_eligible_at = NULL
         WHERE id = ANY($2)",
        release_id,
        &plan.device_ids
    )
    .execute(&mut *tx)
    .await?;
    record_plan(&mut tx, deployment.id, &plan).await?;

    let canary_count = plan.device_ids.len();
    let detail = format!("{canary_count} canary devices");
    record_event(
        &mut tx,
        deployment.id,
        "started",
        Some(user.user_id),
        Some(&detail),
    )
    .await?;

    tx.commit().await?;

    if let Some((distribution, version)) = release_label(release_id, pg_pool).await {
        notify(
            config,
            format!(
                ":rocket: *Deployment Started*\n\n*Distribution:* {distribution}\n*Version:* {version}\n*Canary devices:* {canary_count}\n*Triggered by:* {}",
                user_email.unwrap_or("Unknown")
            ),
        )
        .await;
    }

    Ok(deployment)
}

/// Why the canary can't be promoted yet, independent of who is asking.
async fn promotion_blockers(
    deployment_id: i32,
    conn: &mut PgConnection,
) -> sqlx::Result<Vec<String>> {
    let release = sqlx::query!(
        "SELECT r.distribution_id, r.yanked, r.release_candidate
         FROM deployment d JOIN release r ON r.id = d.release_id
         WHERE d.id = $1",
        deployment_id
    )
    .fetch_one(&mut *conn)
    .await?;

    let mut blockers = Vec::new();
    if release.release_candidate {
        blockers.push("Cannot perform full rollout for a release candidate".to_string());
    }
    if release.yanked {
        blockers.push("This release is yanked.".to_string());
    }

    // The state keeps "updated" once a canary got there, for the history. The gate
    // looks at where each canary is now: one still meant to run the release but not
    // running it hasn't finished, whatever it did before, and one moved elsewhere
    // doesn't count either way.
    let canaries = sqlx::query!(
        r#"SELECT
            COUNT(*) AS "total!",
            COUNT(*) FILTER (WHERE d.release_id = dep.release_id) AS "running!",
            COUNT(*) FILTER (
                WHERE d.target_release_id = dep.release_id
                  AND d.release_id IS DISTINCT FROM dep.release_id
            ) AS "waiting!",
            COUNT(*) FILTER (WHERE s.unhealthy) AS "unhealthy!"
           FROM deployment_device_state s
           JOIN deployment dep ON dep.id = s.deployment_id
           JOIN device d ON d.id = s.device_id
           WHERE s.deployment_id = $1 AND s.wave = 0"#,
        deployment_id
    )
    .fetch_one(&mut *conn)
    .await?;
    if canaries.total == 0 {
        blockers.push("This deployment has no canary devices.".to_string());
    }
    if canaries.total > 0 && canaries.waiting == 0 && canaries.running == 0 {
        blockers.push("No canary device is running this release.".to_string());
    }
    if canaries.waiting > 0 {
        blockers.push(format!(
            "{} of {} canary devices have not finished updating.",
            canaries.waiting, canaries.total
        ));
    }
    if canaries.unhealthy > 0 {
        blockers.push(format!(
            "{} canary devices have unhealthy or unreported services.",
            canaries.unhealthy
        ));
    }

    // Only fleet rollouts run one at a time; a pilot rollout leaves what the fleet
    // follows alone, so it can run alongside one.
    if !release.release_candidate
        && let Some(active) =
            active_fleet_deployment(release.distribution_id, Some(deployment_id), &mut *conn)
                .await?
    {
        blockers.push(conflict_message(&active));
    }
    Ok(blockers)
}

/// Moves a deployment from its canary to the staggered rollout: to the fleet, or
/// for a release candidate to the pilot group picked when it was deployed.
pub async fn promote(
    deployment_id: i32,
    pg_pool: &PgPool,
    config: &Config,
    user: &CurrentUser,
    user_email: Option<&str>,
) -> Result<Deployment, ApiError> {
    policy::ensure_can_ship(user)?;
    let mut tx = pg_pool.begin().await?;

    // Row lock so two promotions racing each other cannot both retarget the fleet.
    let deployment = sqlx::query_as!(
        Deployment,
        r#"SELECT id, release_id, status AS "status!: DeploymentStatus", updated_at, created_at
           FROM deployment WHERE id = $1 FOR UPDATE"#,
        deployment_id
    )
    .fetch_one(&mut *tx)
    .await?;
    let release_id = deployment.release_id;

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
    let blockers = promotion_blockers(deployment_id, &mut tx).await?;
    if !blockers.is_empty() {
        return Err(ApiError::bad_request(format!(
            "Cannot confirm full rollout: {}",
            blockers.join(" ")
        )));
    }

    let distribution_id = sqlx::query_scalar!(
        "SELECT r.distribution_id FROM deployment d JOIN release r ON r.id = d.release_id WHERE d.id = $1",
        deployment_id
    ).fetch_one(&mut *tx).await?;
    let device_ids = sqlx::query_scalar!(
        "SELECT id FROM device WHERE release_id IN (SELECT id FROM release WHERE distribution_id = $1)
         AND follow_latest AND NOT archived FOR UPDATE",
        distribution_id
    ).fetch_all(&mut *tx).await?;

    let updated_deployment = sqlx::query_as!(
        Deployment,
        r#"UPDATE deployment
           SET status = 'done', updated_at = NOW(), promoted_at = NOW(), promoted_by = $2
           WHERE id = $1
           RETURNING id, release_id, status AS "status!: DeploymentStatus", updated_at, created_at"#,
        deployment_id,
        user.user_id
    )
    .fetch_one(&mut *tx)
    .await?;

    let outcome = stagger_retarget(&mut tx, release_id, &device_ids).await?;
    record_plan(&mut tx, deployment_id, &outcome.plan).await?;
    sqlx::query!(
        "UPDATE deployment SET times_out_at = NOW() + make_interval(mins => $2) WHERE id = $1",
        deployment_id,
        timeout_minutes(outcome.window_minutes)
    )
    .execute(&mut *tx)
    .await?;

    // A pilot rollout leaves what the fleet follows alone.
    {
        sqlx::query!(
            "UPDATE distribution SET latest_release_id = $1 WHERE id = $2",
            release_id,
            distribution_id
        )
        .execute(&mut *tx)
        .await?;
    }

    let detail = format!(
        "{} devices over {} minutes",
        outcome.plan.device_ids.len(),
        outcome.window_minutes
    );
    record_event(
        &mut tx,
        deployment_id,
        "promoted",
        Some(user.user_id),
        Some(&detail),
    )
    .await?;

    tx.commit().await?;

    if let Some((distribution, version)) = release_label(release_id, pg_pool).await {
        notify(
            config,
            format!(
                ":white_check_mark: *{} Rollout Confirmed*\n\n*Distribution:* {distribution}\n*Version:* {version}\n*Devices updated:* {}\n*Confirmed by:* {}",
                "Full",
                outcome.retargeted,
                user_email.unwrap_or("Unknown")
            ),
        )
        .await;
    }

    Ok(updated_deployment)
}

pub async fn confirm_full_rollout(
    release_id: i32,
    pg_pool: &PgPool,
    config: &Config,
    user: &CurrentUser,
    user_email: Option<&str>,
) -> Result<Deployment, ApiError> {
    let deployment_id = latest_deployment_id(release_id, pg_pool).await?;
    promote(deployment_id, pg_pool, config, user, user_email).await
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
    user: &CurrentUser,
    user_email: Option<&str>,
) -> Result<RollbackResult, ApiError> {
    policy::ensure_can_ship(user)?;
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

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentState {
    /// Waiting on its canary devices and a person to promote it.
    Canary,
    RollingOut,
    Paused,
    /// Nothing left to wait for. Online devices hold it open until they update;
    /// offline ones are counted but don't.
    Completed,
    Canceled,
    Failed,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentKind {
    /// Canary first, then a staggered rollout: to every device following latest,
    /// or for a release candidate to its pilot group.
    Staged,
    /// Straight to its devices, with no canary phase: Deploy to Selected, or a
    /// small pilot group.
    Direct,
    /// Every device on a yanked release moved to an earlier one at once.
    Rollback,
}

impl DeploymentKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Staged => "staged",
            Self::Direct => "direct",
            Self::Rollback => "rollback",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct DeploymentCreator {
    pub id: i32,
    pub email: Option<String>,
}

/// Everyone who has started a deployment, for filtering by who did it.
pub async fn deployment_creators(pg_pool: &PgPool) -> sqlx::Result<Vec<DeploymentCreator>> {
    sqlx::query_as!(
        DeploymentCreator,
        "SELECT u.id, u.email FROM auth.users u
         WHERE EXISTS (SELECT 1 FROM deployment d WHERE d.created_by = u.id)
         ORDER BY u.email NULLS LAST, u.id"
    )
    .fetch_all(pg_pool)
    .await
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct ActiveDeployment {
    pub id: i32,
    pub release_version: String,
}

/// How many devices are in each state. `unhealthy` overlaps with `updated`: those
/// are devices on the release whose watchdog services are failing.
#[derive(Serialize, Deserialize, Debug, Clone, Default, ToSchema)]
pub struct DeviceCounts {
    pub total: i64,
    pub updated: i64,
    pub updating: i64,
    pub scheduled: i64,
    pub held: i64,
    pub offline: i64,
    pub timed_out: i64,
    pub superseded: i64,
    pub canceled: i64,
    pub unhealthy: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct DeploymentSummary {
    pub id: i32,
    pub kind: DeploymentKind,
    pub state: DeploymentState,
    pub release_id: i32,
    pub release_version: String,
    pub release_candidate: bool,
    pub release_yanked: bool,
    /// This is the deployment that brought the distribution's current latest
    /// release: its most recent confirmed rollout or rollback of that release.
    pub is_latest: bool,
    /// For a rollback, the yanked release its devices were moved off.
    pub rolled_back_from_version: Option<String>,
    pub distribution_id: i32,
    pub distribution_name: String,
    pub distribution_architecture: String,
    /// The distribution's latest release when this started.
    pub base_release_id: Option<i32>,
    pub base_version: Option<String>,
    pub created_at: DateTime<Utc>,
    pub created_by: Option<String>,
    pub created_by_id: Option<i32>,
    pub promoted_at: Option<DateTime<Utc>>,
    pub promoted_by: Option<String>,
    pub paused_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub counts: DeviceCounts,
    /// Canary devices only.
    pub canary: DeviceCounts,
    pub next_wave_at: Option<DateTime<Utc>>,
    /// When the last wave opens (or opened).
    pub last_wave_at: Option<DateTime<Utc>>,
    /// When a rollout still running stops waiting for pending devices and
    /// times them out.
    pub times_out_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct WaveProgress {
    /// 0 is the canary.
    pub wave: i32,
    pub opens_at: DateTime<Utc>,
    pub counts: DeviceCounts,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct DeploymentEventInfo {
    pub id: i64,
    pub kind: String,
    pub user_email: Option<String>,
    pub detail: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// What the signed-in user can do right now, so a page never offers something the
/// API would refuse.
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct DeploymentActions {
    pub can_pause: bool,
    pub can_resume: bool,
    /// Withdraw the release and roll its devices back to where this started.
    pub can_yank: bool,
    pub can_rush: bool,
    /// Every wave has opened, so the run can stop waiting for pending devices.
    pub can_finalize: bool,
    pub can_promote: bool,
    /// Everything standing in the way of promoting.
    pub promote_blockers: Vec<String>,
    /// Why this user can't resume, cancel or promote, if they can't.
    pub denied_reason: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct DeploymentDetail {
    pub summary: DeploymentSummary,
    pub waves: Vec<WaveProgress>,
    pub events: Vec<DeploymentEventInfo>,
    pub actions: DeploymentActions,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct DeploymentDeviceInfo {
    pub device_id: i32,
    pub serial_number: String,
    pub wave: i32,
    pub state: String,
    pub unhealthy_services: Vec<String>,
    pub from_version: Option<String>,
    pub current_version: Option<String>,
    pub opens_at: DateTime<Utc>,
    pub reached_at: Option<DateTime<Utc>>,
    pub last_ping: Option<DateTime<Utc>>,
    #[schema(value_type = HashMap<String, String>)]
    pub labels: SqlxJson<HashMap<String, String>>,
}

fn derive_state(
    status: &DeploymentStatus,
    promoted: bool,
    paused: bool,
    pending: i64,
) -> DeploymentState {
    match status {
        DeploymentStatus::Canceled => DeploymentState::Canceled,
        DeploymentStatus::Failed => DeploymentState::Failed,
        DeploymentStatus::InProgress | DeploymentStatus::Done => {
            if paused {
                DeploymentState::Paused
            } else if !promoted {
                DeploymentState::Canary
            } else if pending > 0 {
                DeploymentState::RollingOut
            } else {
                DeploymentState::Completed
            }
        }
    }
}

/// How long a rollout waits for pending devices once it starts: twice its
/// stagger window, and at least an hour so all-at-once deployments get a fair
/// chance. Some devices following latest are offline or out of reach for good,
/// and without this their rollouts would stay active forever. Those devices
/// still update whenever they get there; the run's result counts them as timed
/// out. Stored as `times_out_at`, which each pause pushes back.
fn timeout_minutes(window_minutes: i32) -> i32 {
    (window_minutes * 2).max(60)
}

/// Freeze each finished run once. `done` means promoted in the legacy schema;
/// scheduled and pending devices must drain before its result can be recorded,
/// unless the rollout timed out.
async fn record_completed_outcomes(pool: &PgPool) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    // SKIP LOCKED so API replicas running this loop split the work instead of queueing.
    let ids = sqlx::query_scalar!(
        "SELECT dep.id FROM deployment dep
         WHERE dep.completed_at IS NULL AND (dep.status IN ('canceled', 'failed') OR (
           dep.status = 'done' AND dep.promoted_at IS NOT NULL
           AND (
               NOT EXISTS (
                   SELECT 1 FROM deployment_device_state s
                   WHERE s.deployment_id = dep.id AND s.state IN ('scheduled', 'updating', 'held')
               )
               OR dep.times_out_at < NOW()
           )))
         ORDER BY dep.id FOR UPDATE OF dep SKIP LOCKED"
    )
    .fetch_all(&mut *tx)
    .await?;
    for id in ids {
        let timed_out = freeze_outcomes(&mut tx, id).await?;
        if timed_out > 0 {
            record_event(
                &mut tx,
                id,
                "timed_out",
                None,
                Some(&timed_out_detail(timed_out)),
            )
            .await?;
        }
    }
    tx.commit().await
}

fn timed_out_detail(count: i64) -> String {
    format!(
        "{count} devices timed out. They keep this release as their target and update on their own when they come online."
    )
}

/// Records each device's result and marks the run completed. Devices that
/// hadn't updated are timed out; returns how many.
async fn freeze_outcomes(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    deployment_id: i32,
) -> sqlx::Result<i64> {
    let timed_out = sqlx::query_scalar!(
        r#"WITH frozen AS (
            UPDATE deployment_devices dd SET
                outcome = CASE WHEN s.state IN ('updated', 'superseded', 'canceled') THEN s.state ELSE 'timed_out' END,
                reached_at = CASE WHEN s.state = 'updated' THEN COALESCE(dd.reached_at, now()) ELSE dd.reached_at END
            FROM deployment_device_state s
            WHERE dd.deployment_id = $1 AND s.deployment_id = dd.deployment_id
              AND s.device_id = dd.device_id
            RETURNING dd.outcome
        )
        SELECT COUNT(*) FILTER (WHERE outcome = 'timed_out') AS "count!" FROM frozen"#,
        deployment_id
    )
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query!(
        "UPDATE deployment SET completed_at = now() WHERE id = $1",
        deployment_id
    )
    .execute(&mut **tx)
    .await?;
    Ok(timed_out)
}

pub fn spawn_completion_recorder(pool: PgPool) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            if let Err(error) = record_completed_outcomes(&pool).await {
                tracing::error!(%error, "Failed to record completed deployment outcomes");
            }
        }
    });
}

struct SummaryFilter {
    id: Option<i32>,
    distribution_id: Option<i32>,
    release_id: Option<i32>,
    rolled_back_from: Option<i32>,
    created_by: Option<i32>,
    kind: Option<DeploymentKind>,
    active: Option<bool>,
    limit: i64,
}

async fn summaries(
    filter: SummaryFilter,
    pg_pool: &PgPool,
) -> sqlx::Result<Vec<DeploymentSummary>> {
    record_completed_outcomes(pg_pool).await?;
    let rows = sqlx::query!(
        r#"SELECT
            d.id, d.kind, d.release_id, d.status AS "status!: DeploymentStatus",
            r.version AS release_version, r.release_candidate, r.yanked AS release_yanked,
            NULL::text AS "rolled_back_from_version?",
            COALESCE(d.id = (
                SELECT MAX(brought.id) FROM deployment brought
                WHERE brought.release_id = dist.latest_release_id
                  AND brought.kind IN ('staged', 'rollback')
                  AND brought.promoted_at IS NOT NULL
                  AND brought.status <> 'canceled'
            ), false) AS "is_latest!",
            r.distribution_id, dist.name AS distribution_name,
            dist.architecture AS distribution_architecture,
            d.base_release_id, base.version AS "base_version?",
            d.created_at, creator.email AS "created_by?", d.created_by AS created_by_id,
            d.promoted_at, promoter.email AS "promoted_by?",
            NULL::timestamptz AS paused_at, d.times_out_at, d.completed_at, d.updated_at,
            COALESCE(c.total, 0) AS "total!",
            COALESCE(c.updated, 0) AS "updated!",
            COALESCE(c.updating, 0) AS "updating!",
            COALESCE(c.scheduled, 0) AS "scheduled!",
            COALESCE(c.held, 0) AS "held!",
            COALESCE(c.offline, 0) AS "offline!",
            COALESCE(c.timed_out, 0) AS "timed_out!",
            COALESCE(c.superseded, 0) AS "superseded!",
            COALESCE(c.canceled, 0) AS "canceled!",
            COALESCE(c.unhealthy, 0) AS "unhealthy!",
            COALESCE(c.canary_total, 0) AS "canary_total!",
            COALESCE(c.canary_updated, 0) AS "canary_updated!",
            COALESCE(c.canary_updating, 0) AS "canary_updating!",
            COALESCE(c.canary_held, 0) AS "canary_held!",
            COALESCE(c.canary_offline, 0) AS "canary_offline!",
            COALESCE(c.canary_timed_out, 0) AS "canary_timed_out!",
            COALESCE(c.canary_superseded, 0) AS "canary_superseded!",
            COALESCE(c.canary_canceled, 0) AS "canary_canceled!",
            COALESCE(c.canary_unhealthy, 0) AS "canary_unhealthy!",
            c.next_wave_at, c.last_wave_at
        FROM deployment d
        JOIN release r ON r.id = d.release_id
        JOIN distribution dist ON dist.id = r.distribution_id
        LEFT JOIN release base ON base.id = d.base_release_id
        LEFT JOIN auth.users creator ON creator.id = d.created_by
        LEFT JOIN auth.users promoter ON promoter.id = d.promoted_by
        LEFT JOIN LATERAL (
            SELECT
                COUNT(*) AS total,
                COUNT(*) FILTER (WHERE s.state = 'updated') AS updated,
                COUNT(*) FILTER (WHERE s.state = 'updating') AS updating,
                COUNT(*) FILTER (WHERE s.state = 'scheduled') AS scheduled,
                COUNT(*) FILTER (WHERE s.state = 'held') AS held,
                COUNT(*) FILTER (WHERE s.state = 'offline') AS offline,
                COUNT(*) FILTER (WHERE s.state = 'timed_out') AS timed_out,
                COUNT(*) FILTER (WHERE s.state = 'superseded') AS superseded,
                COUNT(*) FILTER (WHERE s.state = 'canceled') AS canceled,
                COUNT(*) FILTER (WHERE s.unhealthy) AS unhealthy,
                COUNT(*) FILTER (WHERE s.wave = 0) AS canary_total,
                COUNT(*) FILTER (WHERE s.wave = 0 AND s.state = 'updated') AS canary_updated,
                COUNT(*) FILTER (WHERE s.wave = 0 AND s.state = 'updating') AS canary_updating,
                COUNT(*) FILTER (WHERE s.wave = 0 AND s.state = 'held') AS canary_held,
                COUNT(*) FILTER (WHERE s.wave = 0 AND s.state = 'offline') AS canary_offline,
                COUNT(*) FILTER (WHERE s.wave = 0 AND s.state = 'timed_out') AS canary_timed_out,
                COUNT(*) FILTER (WHERE s.wave = 0 AND s.state = 'superseded') AS canary_superseded,
                COUNT(*) FILTER (WHERE s.wave = 0 AND s.state = 'canceled') AS canary_canceled,
                COUNT(*) FILTER (WHERE s.wave = 0 AND s.unhealthy) AS canary_unhealthy,
                MIN(s.opens_at) FILTER (WHERE s.wave > 0 AND s.opens_at > NOW()) AS next_wave_at,
                MAX(s.opens_at) FILTER (WHERE s.wave > 0) AS last_wave_at
            FROM deployment_device_state s
            WHERE s.deployment_id = d.id
        ) c ON true
        WHERE ($1::int IS NULL OR d.id = $1)
          AND ($2::int IS NULL OR r.distribution_id = $2)
          AND ($3::int IS NULL OR d.release_id = $3)
          AND $5::int IS NULL
          AND ($6::int IS NULL OR d.created_by = $6)
          AND ($7::text IS NULL OR d.kind = $7)
          -- Recorded outcomes are written first, so an unfinished run is one with none.
          AND ($8::bool IS NULL
               OR $8 = (d.status NOT IN ('canceled', 'failed') AND d.completed_at IS NULL))
        ORDER BY d.id DESC
        LIMIT $4"#,
        filter.id,
        filter.distribution_id,
        filter.release_id,
        filter.limit,
        filter.rolled_back_from,
        filter.created_by,
        filter.kind.map(DeploymentKind::as_str),
        filter.active
    )
    .fetch_all(pg_pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let paused = row.paused_at.is_some();
            let state = derive_state(
                &row.status,
                row.promoted_at.is_some(),
                paused,
                row.scheduled + row.updating,
            );
            DeploymentSummary {
                id: row.id,
                kind: match row.kind.as_str() {
                    "direct" => DeploymentKind::Direct,
                    "rollback" => DeploymentKind::Rollback,
                    _ => DeploymentKind::Staged,
                },
                state,
                release_id: row.release_id,
                release_version: row.release_version,
                release_candidate: row.release_candidate,
                release_yanked: row.release_yanked,
                is_latest: row.is_latest,
                rolled_back_from_version: row.rolled_back_from_version,
                distribution_id: row.distribution_id,
                distribution_name: row.distribution_name,
                distribution_architecture: row.distribution_architecture,
                base_release_id: row.base_release_id,
                base_version: row.base_version,
                created_at: row.created_at,
                created_by: row.created_by,
                created_by_id: row.created_by_id,
                promoted_at: row.promoted_at,
                promoted_by: row.promoted_by,
                paused_at: row.paused_at,
                updated_at: row.updated_at,
                completed_at: row.completed_at,
                counts: DeviceCounts {
                    total: row.total,
                    updated: row.updated,
                    updating: row.updating,
                    scheduled: row.scheduled,
                    held: row.held,
                    offline: row.offline,
                    timed_out: row.timed_out,
                    superseded: row.superseded,
                    canceled: row.canceled,
                    unhealthy: row.unhealthy,
                },
                canary: DeviceCounts {
                    total: row.canary_total,
                    updated: row.canary_updated,
                    updating: row.canary_updating,
                    scheduled: 0,
                    held: row.canary_held,
                    offline: row.canary_offline,
                    timed_out: row.canary_timed_out,
                    superseded: row.canary_superseded,
                    canceled: row.canary_canceled,
                    unhealthy: row.canary_unhealthy,
                },
                // A paused deployment's schedule is only a plan until it resumes.
                next_wave_at: row.next_wave_at.filter(|_| !paused),
                last_wave_at: row.last_wave_at.filter(|_| !paused),
                times_out_at: row
                    .times_out_at
                    .filter(|_| row.completed_at.is_none() && !paused),
            }
        })
        .collect())
}

pub struct DeploymentListFilter {
    pub distribution_id: Option<i32>,
    pub release_id: Option<i32>,
    pub rolled_back_from: Option<i32>,
    pub created_by: Option<i32>,
    pub kind: Option<DeploymentKind>,
    /// Only running (canary, rolling out, paused) or only finished runs.
    pub active: Option<bool>,
    pub limit: i64,
}

pub async fn list_deployments(
    filter: DeploymentListFilter,
    pg_pool: &PgPool,
) -> sqlx::Result<Vec<DeploymentSummary>> {
    summaries(
        SummaryFilter {
            id: None,
            distribution_id: filter.distribution_id,
            release_id: filter.release_id,
            rolled_back_from: filter.rolled_back_from,
            created_by: filter.created_by,
            kind: filter.kind,
            active: filter.active,
            limit: filter.limit,
        },
        pg_pool,
    )
    .await
}

pub async fn deployment_detail(
    deployment_id: i32,
    user: &CurrentUser,
    pg_pool: &PgPool,
) -> Result<DeploymentDetail, ApiError> {
    let summary = summaries(
        SummaryFilter {
            id: Some(deployment_id),
            distribution_id: None,
            release_id: None,
            rolled_back_from: None,
            created_by: None,
            kind: None,
            active: None,
            limit: 1,
        },
        pg_pool,
    )
    .await?
    .into_iter()
    .next()
    .ok_or(ApiError::NotFound)?;

    let wave_rows = sqlx::query!(
        r#"SELECT
            wave AS "wave!",
            MIN(opens_at) AS "opens_at!",
            COUNT(*) AS "total!",
            COUNT(*) FILTER (WHERE state = 'updated') AS "updated!",
            COUNT(*) FILTER (WHERE state = 'updating') AS "updating!",
            COUNT(*) FILTER (WHERE state = 'scheduled') AS "scheduled!",
            COUNT(*) FILTER (WHERE state = 'held') AS "held!",
            COUNT(*) FILTER (WHERE state = 'offline') AS "offline!",
            COUNT(*) FILTER (WHERE state = 'timed_out') AS "timed_out!",
            COUNT(*) FILTER (WHERE state = 'superseded') AS "superseded!",
            COUNT(*) FILTER (WHERE state = 'canceled') AS "canceled!",
            COUNT(*) FILTER (WHERE unhealthy) AS "unhealthy!"
        FROM deployment_device_state
        WHERE deployment_id = $1
        GROUP BY wave
        ORDER BY wave"#,
        deployment_id
    )
    .fetch_all(pg_pool)
    .await?;
    let waves = wave_rows
        .into_iter()
        .map(|row| WaveProgress {
            wave: row.wave,
            opens_at: row.opens_at,
            counts: DeviceCounts {
                total: row.total,
                updated: row.updated,
                updating: row.updating,
                scheduled: row.scheduled,
                held: row.held,
                offline: row.offline,
                timed_out: row.timed_out,
                superseded: row.superseded,
                canceled: row.canceled,
                unhealthy: row.unhealthy,
            },
        })
        .collect();

    let events = sqlx::query_as!(
        DeploymentEventInfo,
        r#"SELECT e.id, e.kind, u.email AS "user_email?", e.detail, e.created_at
           FROM deployment_event e
           LEFT JOIN auth.users u ON u.id = e.user_id
           WHERE e.deployment_id = $1
           ORDER BY e.id DESC"#,
        deployment_id
    )
    .fetch_all(pg_pool)
    .await?;

    let mut conn = pg_pool.acquire().await?;
    let denied = policy::cannot_ship(user).map(str::to_string);
    let may_control = denied.is_none();
    // A rollback runs to the end: undoing it would return devices to the yanked release.
    let is_staged = matches!(summary.kind, DeploymentKind::Staged);
    let awaiting_promotion = is_staged
        && summary.promoted_at.is_none()
        && matches!(
            summary.state,
            DeploymentState::Canary | DeploymentState::Paused
        );

    let mut promote_blockers = Vec::new();
    if awaiting_promotion {
        promote_blockers = promotion_blockers(deployment_id, &mut conn).await?;
        if summary.state == DeploymentState::Paused {
            promote_blockers.push("This deployment is paused.".to_string());
        }
    }

    let actions = DeploymentActions {
        can_pause: false,
        can_resume: false,
        can_yank: false,
        can_rush: false,
        can_finalize: false,
        can_promote: awaiting_promotion && may_control && promote_blockers.is_empty(),
        promote_blockers,
        denied_reason: denied,
    };

    Ok(DeploymentDetail {
        summary,
        waves,
        events,
        actions,
    })
}

/// Which of a deployment's devices to list. A large fleet has thousands, and
/// the page only ever shows a sample, so the filters run here rather than over
/// a full download.
pub struct DeploymentDeviceFilter {
    /// A device state, or `unhealthy`.
    pub state: Option<String>,
    pub wave: Option<i32>,
    /// Part of a serial number.
    pub search: Option<String>,
    pub limit: i64,
}

pub async fn deployment_devices(
    deployment_id: i32,
    filter: DeploymentDeviceFilter,
    pg_pool: &PgPool,
) -> sqlx::Result<Vec<DeploymentDeviceInfo>> {
    sqlx::query_as!(
        DeploymentDeviceInfo,
        r#"SELECT
            s.device_id AS "device_id!",
            d.serial_number,
            (SELECT COALESCE(JSONB_OBJECT_AGG(l.name, dl.value), '{}'::jsonb)
             FROM device_label dl JOIN label l ON l.id = dl.label_id
             WHERE dl.device_id = d.id) AS "labels!: SqlxJson<HashMap<String, String>>",

            s.wave AS "wave!",
            s.state AS "state!",
            ARRAY(
                SELECT rs.service_name FROM release_services rs
                WHERE s.unhealthy
                  AND rs.release_id = dep.release_id
                  AND rs.watchdog_sec IS NOT NULL
                  AND NOT EXISTS (
                      SELECT 1 FROM device_service_status dss
                      WHERE dss.device_id = d.id
                        AND dss.release_service_id = rs.id
                        AND dss.active_state = 'active'
                  )
                ORDER BY rs.service_name
            ) AS "unhealthy_services!",
            from_release.version AS "from_version?",
            current_release.version AS "current_version?",
            s.opens_at AS "opens_at!",
            s.reached_at,
            d.last_ping
        FROM deployment_device_state s
        JOIN deployment dep ON dep.id = s.deployment_id
        JOIN device d ON d.id = s.device_id
        LEFT JOIN release from_release ON from_release.id = s.from_release_id
        LEFT JOIN release current_release ON current_release.id = d.release_id
        WHERE s.deployment_id = $1
          AND ($2::text IS NULL OR s.state = $2 OR ($2 = 'unhealthy' AND s.unhealthy))
          AND ($3::int IS NULL OR s.wave = $3)
          AND ($4::text IS NULL OR d.serial_number ILIKE '%' || $4 || '%')
        ORDER BY s.wave, d.serial_number
        LIMIT $5"#,
        deployment_id,
        filter.state,
        filter.wave,
        filter.search,
        filter.limit
    )
    .fetch_all(pg_pool)
    .await
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
        "WITH waiting AS (
             SELECT id FROM device
             WHERE target_release_id = $1 AND target_release_eligible_at > NOW()
               AND release_id IS DISTINCT FROM target_release_id AND NOT archived
             FOR UPDATE
         ), opened AS (
             UPDATE deployment_devices dd SET opens_at = NOW()
             FROM waiting, deployment dep
             WHERE dd.device_id = waiting.id AND dep.id = dd.deployment_id
               AND dep.release_id = $1 AND dep.completed_at IS NULL AND dd.opens_at > NOW()
             RETURNING dd.device_id
         )
         UPDATE device d SET target_release_eligible_at = NOW()
         FROM waiting WHERE d.id = waiting.id",
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

/// The canary devices of the release's most recent deployment.
pub async fn get_devices_in_deployment(
    release_id: i32,
    pg_pool: &PgPool,
) -> anyhow::Result<Vec<DeploymentDeviceWithStatus>> {
    let deployment = sqlx::query!(
        "SELECT id FROM deployment WHERE release_id = $1 ORDER BY id DESC LIMIT 1",
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
            WHERE dd.deployment_id = $1 AND dd.wave = 0
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

/// Watchdog services on the canary devices of the release's most recent deployment.
pub async fn get_deployment_service_health(
    release_id: i32,
    pg_pool: &PgPool,
) -> anyhow::Result<Vec<DeviceServiceHealth>> {
    let deployment = sqlx::query!(
        "SELECT id FROM deployment WHERE release_id = $1 ORDER BY id DESC LIMIT 1",
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
          AND dd.wave = 0
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

    /// A deployment past its canary, with `count` devices spread over its waves.
    async fn rolling_deployment(
        pool: &PgPool,
        count: usize,
    ) -> anyhow::Result<(i32, i32, i32, Vec<i32>)> {
        let [current, target] = insert_releases(pool).await?;
        let ids = insert_devices(pool, count, Some(current)).await?;
        let deployment_id: i32 = sqlx::query_scalar(
            "INSERT INTO deployment (release_id, status, promoted_at, base_release_id)
             VALUES ($1, 'done', NOW(), $2) RETURNING id",
        )
        .bind(target)
        .bind(current)
        .fetch_one(pool)
        .await?;
        let mut tx = pool.begin().await?;
        let outcome = stagger_retarget(&mut tx, target, &ids).await?;
        record_plan(&mut tx, deployment_id, &outcome.plan).await?;
        tx.commit().await?;
        Ok((deployment_id, current, target, ids))
    }

    async fn state_counts(
        pool: &PgPool,
        deployment_id: i32,
    ) -> anyhow::Result<HashMap<String, i64>> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT state, COUNT(*) FROM deployment_device_state WHERE deployment_id = $1 GROUP BY state",
        )
        .bind(deployment_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().collect())
    }

    /// A deployment of `release` whose one canary, `canary`, already runs it.
    async fn passed_canary(
        pool: &PgPool,
        release: i32,
        base: i32,
        canary: i32,
    ) -> anyhow::Result<i32> {
        sqlx::query("UPDATE device SET release_id = $1, target_release_id = $1 WHERE id = $2")
            .bind(release)
            .bind(canary)
            .execute(pool)
            .await?;
        let deployment_id: i32 = sqlx::query_scalar(
            "INSERT INTO deployment (release_id, status, base_release_id)
             VALUES ($1, 'in_progress', $2) RETURNING id",
        )
        .bind(release)
        .bind(base)
        .fetch_one(pool)
        .await?;
        let mut tx = pool.begin().await?;
        let plan = RolloutPlan {
            waves: vec![0],
            device_ids: vec![canary],
        };
        record_plan(&mut tx, deployment_id, &plan).await?;
        tx.commit().await?;
        Ok(deployment_id)
    }

    async fn targets(pool: &PgPool, ids: &[i32]) -> anyhow::Result<HashMap<i32, Option<i32>>> {
        let rows: Vec<(i32, Option<i32>)> =
            sqlx::query_as("SELECT id, target_release_id FROM device WHERE id = ANY($1)")
                .bind(ids)
                .fetch_all(pool)
                .await?;
        Ok(rows.into_iter().collect())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn a_rollout_stops_waiting_for_pending_devices_after_the_timeout(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let (deployment_id, _, target, ids) = rolling_deployment(&pool, 3).await?;
        // Online but never updating: pending for good.
        sqlx::query(
            "UPDATE device SET last_ping = NOW(), target_release_eligible_at = NULL WHERE id = ANY($1)",
        )
        .bind(&ids)
        .execute(&pool)
        .await?;
        let completed = || async {
            sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
                "SELECT completed_at FROM deployment WHERE id = $1",
            )
            .bind(deployment_id)
            .fetch_one(&pool)
            .await
        };

        let set_deadline = |minutes: i32| {
            sqlx::query(
                "UPDATE deployment SET times_out_at = NOW() + make_interval(mins => $2) WHERE id = $1",
            )
            .bind(deployment_id)
            .bind(minutes)
            .execute(&pool)
        };
        let listed = |active: bool| {
            list_deployments(
                DeploymentListFilter {
                    distribution_id: None,
                    release_id: None,
                    rolled_back_from: None,
                    created_by: None,
                    kind: None,
                    active: Some(active),
                    limit: 5,
                },
                &pool,
            )
        };
        set_deadline(10).await?;
        record_completed_outcomes(&pool).await?;
        assert!(completed().await?.is_none(), "still within the timeout");
        assert!(listed(true).await?.iter().any(|d| d.id == deployment_id));

        set_deadline(-1).await?;
        record_completed_outcomes(&pool).await?;
        assert!(completed().await?.is_some(), "timed out");
        assert!(listed(false).await?.iter().any(|d| d.id == deployment_id));
        assert!(listed(true).await?.is_empty());
        assert_eq!(
            state_counts(&pool, deployment_id).await?.get("timed_out"),
            Some(&3)
        );
        let still_targeted = targets(&pool, &ids).await?;
        assert!(
            ids.iter()
                .all(|id| still_targeted.get(id) == Some(&Some(target))),
            "they still update when they get there"
        );
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn the_canary_gate_checks_where_canaries_are_now(pool: PgPool) -> anyhow::Result<()> {
        let [current, target] = insert_releases(&pool).await?;
        let reverted = insert_devices(&pool, 1, Some(current)).await?[0];
        let deployment_id = passed_canary(&pool, target, current, reverted).await?;
        let staying = insert_devices(&pool, 1, Some(target)).await?;
        sqlx::query("UPDATE device SET target_release_id = release_id WHERE id = $1")
            .bind(staying[0])
            .execute(&pool)
            .await?;
        let mut tx = pool.begin().await?;
        let plan = RolloutPlan {
            waves: vec![0],
            device_ids: staying.clone(),
        };
        record_plan(&mut tx, deployment_id, &plan).await?;
        tx.commit().await?;
        let mut conn = pool.acquire().await?;
        assert!(
            promotion_blockers(deployment_id, &mut conn)
                .await?
                .is_empty()
        );

        // One canary got there, which the history keeps, then went back while
        // still meant to run the release: the gate holds despite the other.
        sqlx::query("UPDATE deployment_devices SET reached_at = NOW() WHERE device_id = $1")
            .bind(reverted)
            .execute(&pool)
            .await?;
        sqlx::query("UPDATE device SET release_id = $1 WHERE id = $2")
            .bind(current)
            .bind(reverted)
            .execute(&pool)
            .await?;
        assert_eq!(
            state_counts(&pool, deployment_id).await?.get("updated"),
            Some(&2)
        );
        let blockers = promotion_blockers(deployment_id, &mut conn).await?;
        assert!(
            blockers
                .iter()
                .any(|b| b.contains("1 of 2 canary devices have not finished")),
            "{blockers:?}"
        );

        // Moved elsewhere, it no longer counts either way; with every canary moved
        // elsewhere nothing is left running the release.
        let move_elsewhere = |id: i32| {
            sqlx::query("UPDATE device SET target_release_id = release_id WHERE id = $1")
                .bind(id)
                .execute(&pool)
        };
        move_elsewhere(reverted).await?;
        assert!(
            promotion_blockers(deployment_id, &mut conn)
                .await?
                .is_empty()
        );
        sqlx::query("UPDATE device SET release_id = $1 WHERE id = $2")
            .bind(current)
            .bind(staying[0])
            .execute(&pool)
            .await?;
        move_elsewhere(staying[0]).await?;
        let blockers = promotion_blockers(deployment_id, &mut conn).await?;
        assert!(
            blockers
                .iter()
                .any(|b| b.contains("No canary device is running")),
            "{blockers:?}"
        );
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn deployment_device_list_includes_labels_without_duplicating_devices(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let (deployment_id, _, _, ids) = rolling_deployment(&pool, 2).await?;
        sqlx::query(
            "WITH labels AS (INSERT INTO label (name) VALUES ('site'), ('demo') RETURNING id, name)
            INSERT INTO device_label (device_id, label_id, value)
            SELECT $1, id, CASE WHEN name = 'site' THEN 'Copenhagen' ELSE '' END FROM labels",
        )
        .bind(ids[0])
        .execute(&pool)
        .await?;
        let all = || DeploymentDeviceFilter {
            state: None,
            wave: None,
            search: None,
            limit: 100,
        };
        let devices = deployment_devices(deployment_id, all(), &pool).await?;
        assert_eq!(devices.len(), 2);
        let capped = deployment_devices(
            deployment_id,
            DeploymentDeviceFilter { limit: 1, ..all() },
            &pool,
        )
        .await?;
        assert_eq!(capped.len(), 1);
        let serial = devices
            .iter()
            .find(|d| d.device_id == ids[1])
            .map(|d| d.serial_number.clone())
            .ok_or_else(|| anyhow::anyhow!("missing device"))?;
        let found = deployment_devices(
            deployment_id,
            DeploymentDeviceFilter {
                search: Some(serial[..8].to_uppercase()),
                ..all()
            },
            &pool,
        )
        .await?;
        assert_eq!(
            found.iter().map(|d| d.device_id).collect::<Vec<_>>(),
            vec![ids[1]]
        );
        let labeled = devices
            .iter()
            .find(|d| d.device_id == ids[0])
            .ok_or_else(|| anyhow::anyhow!("missing labeled device"))?;
        assert_eq!(
            labeled.labels.get("site").map(String::as_str),
            Some("Copenhagen")
        );
        assert_eq!(labeled.labels.get("demo").map(String::as_str), Some(""));
        assert!(
            devices
                .iter()
                .any(|d| d.device_id == ids[1] && d.labels.is_empty())
        );
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn completed_outcomes_survive_device_health_and_release_changes(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let (deployment_id, current, target, ids) = rolling_deployment(&pool, 2).await?;
        // Both devices start online, so an active run must not be frozen.
        sqlx::query("UPDATE device SET last_ping = now() WHERE id = ANY($1)")
            .bind(&ids)
            .execute(&pool)
            .await?;
        record_completed_outcomes(&pool).await?;
        let completed: bool =
            sqlx::query_scalar("SELECT completed_at IS NOT NULL FROM deployment WHERE id=$1")
                .bind(deployment_id)
                .fetch_one(&pool)
                .await?;
        assert!(!completed);
        // One confirms success; the other never checks in to update.
        crate::device::save_release_id(ids[0], Some(target), &pool).await?;
        sqlx::query("UPDATE device SET last_ping = now() - INTERVAL '1 day' WHERE id = ANY($1)")
            .bind(&ids)
            .execute(&pool)
            .await?;
        record_completed_outcomes(&pool).await?;
        let before = state_counts(&pool, deployment_id).await?;
        assert_eq!(before.get("updated"), Some(&1));
        assert_eq!(before.get("timed_out"), Some(&1));
        // Subsequent deployments and late check-ins cannot rewrite this run.
        crate::device::save_release_id(ids[0], Some(current), &pool).await?;
        crate::device::save_release_id(ids[1], Some(target), &pool).await?;
        sqlx::query("UPDATE device SET last_ping = now(), archived = true WHERE id = ANY($1)")
            .bind(&ids)
            .execute(&pool)
            .await?;
        record_completed_outcomes(&pool).await?;
        assert_eq!(state_counts(&pool, deployment_id).await?, before);
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn progress_and_finish_now_follow_the_buckets(pool: PgPool) -> anyhow::Result<()> {
        let (deployment_id, _current, target, ids) = rolling_deployment(&pool, 20).await?;
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
        let still_waiting: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                SELECT 1 FROM deployment_devices dd JOIN device d ON d.id = dd.device_id
                WHERE dd.deployment_id = $1 AND d.release_id IS DISTINCT FROM d.target_release_id
                  AND dd.opens_at > NOW()
            )",
        )
        .bind(deployment_id)
        .fetch_one(&pool)
        .await?;
        assert!(!still_waiting, "recorded waves open with their devices");
        Ok(())
    }
}
