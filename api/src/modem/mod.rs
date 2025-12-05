use models::modem::Modem;
use sqlx::PgPool;
use tracing::error;

pub mod route;

pub async fn save_modem(
    serial_number: String,
    imei: String,
    network_provider: String,
    pool: &PgPool,
) -> anyhow::Result<Modem> {
    let mut tx = pool.begin().await?;

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
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to save modem info {err}");
        anyhow::anyhow!("Failed to save modem info")
    })?;

    sqlx::query!(
        "UPDATE device SET modem_id = NULL WHERE modem_id = $1",
        modem.id
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!(
            "Failed to clear old modem assignment for modem_id {}; {err}",
            modem.id
        );
        anyhow::anyhow!("Failed to clear old modem assignment")
    })?;

    sqlx::query!(
        "UPDATE device SET modem_id = $1 WHERE serial_number = $2",
        modem.id,
        serial_number
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to update device {serial_number} modem_id; {err}");
        anyhow::anyhow!("Failed to update device info")
    })?;

    tx.commit().await?;
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
