use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use sqlx::types::chrono;
use utoipa::ToSchema;

pub mod route;

#[derive(sqlx::Type, Serialize, Deserialize, Debug, Clone, ToSchema, PartialEq)]
#[sqlx(type_name = "deployment_status", rename_all = "snake_case")]
pub enum DeploymentStatus {
    InProgress,
    Failed,
    Canceled,
    Done,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct Deployment {
    pub id: i32,
    pub release_id: i32,
    pub status: DeploymentStatus,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct DeploymentDeviceWithStatus {
    pub device_id: i32,
    pub serial_number: String,
    pub release_id: Option<i32>,
    pub target_release_id: Option<i32>,
    pub last_ping: Option<chrono::DateTime<chrono::Utc>>,
    pub added_at: chrono::DateTime<chrono::Utc>,
}

impl Deployment {
    pub async fn get(release_id: i32, pg_pool: &PgPool) -> anyhow::Result<Option<Self>> {
        Ok(sqlx::query_as!(
            Self,
            r#"
            SELECT id, release_id, status AS "status!: DeploymentStatus", updated_at, created_at
            FROM deployment WHERE release_id = $1
            "#,
            release_id
        )
        .fetch_optional(pg_pool)
        .await?)
    }

    pub async fn new(release_id: i32, pg_pool: &PgPool) -> anyhow::Result<Self> {
        // Get the distribution_id for this release
        let release = sqlx::query!(
            "SELECT distribution_id FROM release WHERE id = $1",
            release_id
        )
        .fetch_one(pg_pool)
        .await?;

        let deployment = sqlx::query_as!(
            Self,
            r#"
    INSERT INTO deployment (release_id, status)
    VALUES ($1, 'in_progress')
    RETURNING id, release_id, status AS "status!: DeploymentStatus", updated_at, created_at
    "#,
            release_id
        )
        .fetch_one(pg_pool)
        .await?;

        let mut tx = pg_pool.begin().await?;

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
        .await?;

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

    pub async fn check_done(release_id: i32, pg_pool: &PgPool) -> anyhow::Result<Self> {
        let mut tx = pg_pool.begin().await?;

        // First, get the deployment for the release_id
        let deployment = sqlx::query!(
        "SELECT id, release_id, status AS \"status!: DeploymentStatus\" FROM deployment WHERE release_id = $1",
        release_id
    )
          .fetch_one(&mut *tx)
          .await?;

        if deployment.status == DeploymentStatus::Done {
            let deployment_obj = sqlx::query_as!(
        Self,
        "SELECT id, release_id, status AS \"status!: DeploymentStatus\", updated_at, created_at
         FROM deployment WHERE id = $1",
        deployment.id
    )
            .fetch_one(&mut *tx)
            .await?;

            tx.commit().await?;
            return Ok(deployment_obj);
        }

        // Get the distribution_id for this release
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

        // If there are no devices to update, we can't mark as done
        if device_ids.is_empty() {
            let deployment_obj = sqlx::query_as!(
            Self,
            "SELECT id, release_id, status AS \"status!: DeploymentStatus\", updated_at, created_at
             FROM deployment WHERE id = $1",
            deployment.id
        )
            .fetch_one(&mut *tx)
            .await?;

            tx.commit().await?;
            return Ok(deployment_obj);
        }

        // Check if all deployment devices have their release_id matching their target_release
        // This counts devices where release_id != target_release_id
        let mismatched_devices_count = sqlx::query_scalar!(
            "SELECT COUNT(*)
         FROM device
         WHERE id = ANY($1) AND release_id != target_release_id",
            &device_ids
        )
        .fetch_one(&mut *tx)
        .await?;

        // If any devices have mismatched release_id and target_release_id, return the current deployment without changes
        if mismatched_devices_count.unwrap_or(0) > 0 {
            let deployment_obj = sqlx::query_as!(
            Self,
            "SELECT id, release_id, status AS \"status!: DeploymentStatus\", updated_at, created_at
             FROM deployment WHERE id = $1",
            deployment.id
        )
            .fetch_one(&mut *tx)
            .await?;

            tx.commit().await?;
            return Ok(deployment_obj);
        }

        // All canary devices are updated, return the current deployment
        // User must manually confirm to proceed with full rollout
        let deployment_obj = sqlx::query_as!(
            Self,
            "SELECT id, release_id, status AS \"status!: DeploymentStatus\", updated_at, created_at
             FROM deployment WHERE id = $1",
            deployment.id
        )
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(deployment_obj)
    }

    pub async fn confirm_full_rollout(
        release_id: i32,
        pg_pool: &PgPool,
        slack_hook_url: Option<&str>,
    ) -> anyhow::Result<Self> {
        let mut tx = pg_pool.begin().await?;

        let deployment = sqlx::query!(
            "SELECT id, release_id, status AS \"status!: DeploymentStatus\" FROM deployment WHERE release_id = $1",
            release_id
        )
        .fetch_one(&mut *tx)
        .await?;

        if deployment.status == DeploymentStatus::Done {
            let deployment_obj = sqlx::query_as!(
                Self,
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

        let mismatched_devices_count = sqlx::query_scalar!(
            "SELECT COUNT(*)
             FROM device
             WHERE id = ANY($1) AND release_id != target_release_id",
            &device_ids
        )
        .fetch_one(&mut *tx)
        .await?;

        if mismatched_devices_count.unwrap_or(0) > 0 {
            anyhow::bail!(
                "Cannot confirm full rollout: canary devices have not completed updating"
            );
        }

        let updated_deployment = sqlx::query_as!(
            Self,
            "UPDATE deployment SET status = 'done'
             WHERE release_id = $1
             RETURNING id, release_id, status AS \"status!: DeploymentStatus\", updated_at, created_at",
            release_id
        )
        .fetch_one(&mut *tx)
        .await?;

        let device_count = sqlx::query_scalar!(
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

        if let Some(hook_url) = slack_hook_url {
            let release_info = sqlx::query!(
                "SELECT r.version, d.name as distribution_name, d.architecture as distribution_architecture
                 FROM release r
                 JOIN distribution d ON r.distribution_id = d.id
                 WHERE r.id = $1",
                release_id
            )
            .fetch_one(pg_pool)
            .await?;

            let total_devices = device_count.unwrap_or(0);

            let message = json!({
                "text": format!("Full rollout confirmed for release v{}", release_info.version),
                "blocks": [
                    {
                        "type": "section",
                        "text": {
                            "type": "mrkdwn",
                            "text": format!("*Full Rollout Started*\n\n*Release:* v{}\n*Distribution:* {} ({})\n*Target Devices:* {}\n\nThe release is now rolling out to all devices in the distribution.",
                                release_info.version,
                                release_info.distribution_name,
                                release_info.distribution_architecture,
                                total_devices
                            )
                        }
                    }
                ]
            });

            let hook_url_owned = hook_url.to_string();
            tokio::spawn(async move {
                let client = reqwest::Client::new();
                let _res = client
                    .post(&hook_url_owned)
                    .header("Content-Type", "application/json")
                    .json(&message)
                    .send()
                    .await
                    .inspect_err(|e| tracing::error!("Failed to send Slack notification: {}", e));
            });
        }

        Ok(updated_deployment)
    }

    pub async fn get_devices(
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
                dd.created_at AS added_at
            FROM deployment_devices dd
            JOIN device d ON dd.device_id = d.id
            WHERE dd.deployment_id = $1
            ORDER BY dd.created_at ASC
            "#,
            deployment.id
        )
        .fetch_all(pg_pool)
        .await?;

        Ok(devices)
    }
}
