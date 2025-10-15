use serde::Serialize;
use sqlx::PgPool;
use sqlx::types::chrono;
use tracing::error;

pub mod route;

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Modem {
    pub id: i32,
    pub imei: String,
    pub network_provider: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl Modem {
    pub async fn save_modem(
        serial_number: String,
        imei: String,
        network_provider: String,
        pool: &PgPool,
    ) -> anyhow::Result<Self> {
        let modem = sqlx::query_as!(
            Modem,
            "
            INSERT INTO modem (imei, network_provider, updated_at)
             VALUES ($1, $2, NOW())
             ON CONFLICT (imei) DO UPDATE SET network_provider = $2, updated_at = NOW()
             RETURNING *;
             ",
            imei,
            network_provider,
        )
        .fetch_one(pool)
        .await
        .map_err(|err| {
            error!("Failed to save modem info {err}");
            anyhow::anyhow!("Failed to save modem info")
        })?;
        sqlx::query!(
            "UPDATE device SET modem_id = $1 WHERE serial_number = $2",
            modem.id,
            serial_number
        )
        .execute(pool)
        .await
        .map_err(|err| {
            error!("Failed to update device {serial_number} modem_id; {err}");
            anyhow::anyhow!("Failed to update device info")
        })?;
        Ok(modem)
    }

    pub async fn clear_modem(serial_number: String, pool: &PgPool) -> anyhow::Result<()> {
        sqlx::query!(
            "UPDATE device SET modem_id = NULL WHERE serial_number = $1",
            serial_number
        )
        .execute(pool)
        .await
        .map_err(|err| {
            error!("Failed to clear device {serial_number} modem_id; {err}");
            anyhow::anyhow!("Failed to clear device modem association")
        })?;
        Ok(())
    }
}
