use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;

pub mod route;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct Dashboard {
    pub total_count: u32,
    pub online_count: u32,
    pub offline_count: u32,
    pub outdated_count: u32,
    pub archived_count: u32,
}

impl Dashboard {
    pub async fn new(pool: &PgPool) -> Self {
        let total_count = sqlx::query_scalar!("SELECT COUNT(*) FROM device WHERE archived = false")
            .fetch_one(pool)
            .await
            .unwrap_or(Some(0))
            .unwrap_or(0) as u32;

        let online_count = sqlx::query_scalar!(
      "SELECT COUNT(*) FROM device WHERE last_ping >= now() - INTERVAL '5 minutes' AND archived = false"
    )
      .fetch_one(pool)
      .await
      .unwrap_or(Some(0))
      .unwrap_or(0) as u32;

        let offline_count = sqlx::query_scalar!(
      "SELECT COUNT(*) FROM device WHERE last_ping < now() - INTERVAL '5 minutes' AND archived = false"
    )
      .fetch_one(pool)
      .await
      .unwrap_or(Some(0))
      .unwrap_or(0) as u32;

        let outdated_count = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM device WHERE release_id != target_release_id AND archived = false"
        )
        .fetch_one(pool)
        .await
        .unwrap_or(Some(0))
        .unwrap_or(0) as u32;

        let archived_count =
            sqlx::query_scalar!("SELECT COUNT(*) FROM device WHERE archived = true")
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
