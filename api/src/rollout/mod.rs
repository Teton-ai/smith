pub mod route;

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct DistributionRolloutStats {
    pub distribution_id: i32,
    pub total_devices: Option<i64>,
    pub updated_devices: Option<i64>,
    pub pending_devices: Option<i64>,
}

pub async fn get_distribution_rollout_stats(
    distribution_id: i32,
    pg_pool: &PgPool,
) -> anyhow::Result<DistributionRolloutStats> {
    let result = match sqlx::query_as!(
            DistributionRolloutStats,
            "
            SELECT
                r.distribution_id,
                COALESCE(COUNT(*), 0) as total_devices,
                COALESCE(COUNT(*) FILTER (WHERE d.release_id = d.target_release_id), 0) as updated_devices,
                COALESCE(COUNT(*) FILTER (WHERE d.release_id != d.target_release_id), 0) as pending_devices
            FROM device d
            JOIN release r ON d.target_release_id = r.id
            WHERE d.target_release_id IS NOT NULL
            AND r.distribution_id = $1
            GROUP BY r.distribution_id
            ",
            distribution_id
        )
          .fetch_optional(pg_pool)
          .await? {
            Some(r) => r,
            None => DistributionRolloutStats {
                distribution_id,
                total_devices: Some(0),
                updated_devices: Some(0),
                pending_devices: Some(0),
            },
        };
    Ok(result)
}

pub async fn get_distributions_rollout_stats(
    pg_pool: &PgPool,
) -> anyhow::Result<Vec<DistributionRolloutStats>> {
    let result = sqlx::query_as!(
            DistributionRolloutStats,
            "
            SELECT
                r.distribution_id,
                COALESCE(COUNT(*), 0) as total_devices,
                COALESCE(COUNT(*) FILTER (WHERE d.release_id = d.target_release_id), 0) as updated_devices,
                COALESCE(COUNT(*) FILTER (WHERE d.release_id != d.target_release_id), 0) as pending_devices
            FROM device d
            JOIN release r ON d.target_release_id = r.id
            GROUP BY r.distribution_id
            "
        )
          .fetch_all(pg_pool)
          .await?;
    Ok(result)
}
