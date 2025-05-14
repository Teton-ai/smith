use crate::deployment::schema::{Deployment, DeploymentStatus};
use sqlx::PgPool;

pub mod routes;
pub mod schema;

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
    WHERE d.last_ping > NOW() - INTERVAL '5 minutes'
    AND d.release_id = d.target_release_id
    AND r.distribution_id = $1
    ORDER BY d.last_ping DESC LIMIT 10
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

        // All devices are updated, update the deployment status to 'done'
        let updated_deployment = sqlx::query_as!(
            Self,
            "
        UPDATE deployment SET status = 'done'
        WHERE release_id = $1
        RETURNING id, release_id, status AS \"status!: DeploymentStatus\", updated_at, created_at
        ",
            release_id
        )
        .fetch_one(&mut *tx)
        .await?;

        // Update all devices within the same distribution to have target_release_id = release_id
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

        // Commit the transaction
        tx.commit().await?;
        Ok(updated_deployment)
    }
}
