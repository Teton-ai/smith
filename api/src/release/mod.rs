use serde::{Deserialize, Serialize};
use sqlx::types::chrono;

pub mod route;

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Release {
    pub id: i32,
    pub distribution_id: i32,
    pub distribution_architecture: String,
    pub distribution_name: String,
    pub version: String,
    pub draft: bool,
    pub yanked: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub user_id: Option<i32>,
}

impl Release {
    pub async fn get_release_by_id(
        release_id: i32,
        pg_pool: &sqlx::PgPool,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            Release,
            "
        SELECT release.*,
        distribution.name AS distribution_name,
        distribution.architecture AS distribution_architecture
        FROM release
        JOIN distribution ON release.distribution_id = distribution.id
        WHERE release.id = $1
        ",
            release_id
        )
        .fetch_optional(pg_pool)
        .await
    }

    pub async fn get_latest_distribution_release(
        distribution_id: i32,
        pg_pool: &sqlx::PgPool,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as!(
            Release,
            "
        SELECT release.*,
        distribution.name AS distribution_name,
        distribution.architecture AS distribution_architecture
        FROM release
        JOIN distribution ON release.distribution_id = distribution.id
        WHERE distribution_id = $1
        AND draft = false
        AND yanked = FALSE
        ORDER BY created_at DESC LIMIT 1
        ",
            distribution_id
        )
        .fetch_one(pg_pool)
        .await
    }
}
