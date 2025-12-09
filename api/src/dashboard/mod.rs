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
    pub async fn new(pool: &PgPool, excluded_labels: &[String]) -> Self {
        let total_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) FROM device d
            WHERE d.archived = false
            AND NOT EXISTS (
                SELECT 1 FROM device_label dl
                JOIN label l ON l.id = dl.label_id
                WHERE dl.device_id = d.id
                AND l.name || '=' || dl.value = ANY($1)
            )
            "#,
            excluded_labels
        )
        .fetch_one(pool)
        .await
        .unwrap_or(Some(0))
        .unwrap_or(0) as u32;

        let online_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) FROM device d
            WHERE d.last_ping >= now() - INTERVAL '5 minutes'
            AND d.archived = false
            AND NOT EXISTS (
                SELECT 1 FROM device_label dl
                JOIN label l ON l.id = dl.label_id
                WHERE dl.device_id = d.id
                AND l.name || '=' || dl.value = ANY($1)
            )
            "#,
            excluded_labels
        )
        .fetch_one(pool)
        .await
        .unwrap_or(Some(0))
        .unwrap_or(0) as u32;

        let offline_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) FROM device d
            WHERE d.last_ping < now() - INTERVAL '5 minutes'
            AND d.archived = false
            AND NOT EXISTS (
                SELECT 1 FROM device_label dl
                JOIN label l ON l.id = dl.label_id
                WHERE dl.device_id = d.id
                AND l.name || '=' || dl.value = ANY($1)
            )
            "#,
            excluded_labels
        )
        .fetch_one(pool)
        .await
        .unwrap_or(Some(0))
        .unwrap_or(0) as u32;

        let outdated_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) FROM device d
            WHERE d.release_id != d.target_release_id
            AND d.archived = false
            AND NOT EXISTS (
                SELECT 1 FROM device_label dl
                JOIN label l ON l.id = dl.label_id
                WHERE dl.device_id = d.id
                AND l.name || '=' || dl.value = ANY($1)
            )
            "#,
            excluded_labels
        )
        .fetch_one(pool)
        .await
        .unwrap_or(Some(0))
        .unwrap_or(0) as u32;

        let archived_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) FROM device d
            WHERE d.archived = true
            AND NOT EXISTS (
                SELECT 1 FROM device_label dl
                JOIN label l ON l.id = dl.label_id
                WHERE dl.device_id = d.id
                AND l.name || '=' || dl.value = ANY($1)
            )
            "#,
            excluded_labels
        )
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
