use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;

pub mod route;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct RegistrationCount {
    pub date: NaiveDate,
    pub count: i64,
}

pub async fn get_registration_counts(
    pool: &PgPool,
    excluded_labels: &[String],
) -> Result<Vec<RegistrationCount>, sqlx::Error> {
    sqlx::query_as!(
        RegistrationCount,
        r#"
        SELECT DATE_TRUNC('month', created_on)::date as "date!", COUNT(*) as "count!"
        FROM device d
        WHERE NOT EXISTS (
            SELECT 1 FROM device_label dl
            JOIN label l ON l.id = dl.label_id
            WHERE CARDINALITY($1::text[]) > 0
              AND dl.device_id = d.id
              AND l.name || '=' || dl.value = ANY($1)
        )
        GROUP BY DATE_TRUNC('month', created_on)
        ORDER BY DATE_TRUNC('month', created_on)
        "#,
        excluded_labels
    )
    .fetch_all(pool)
    .await
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct Dashboard {
    pub total_count: u32,
    pub online_count: u32,
    pub offline_count: u32,
    pub outdated_count: u32,
    pub archived_count: u32,
}

impl Dashboard {
    pub async fn new(pool: &PgPool, excluded_labels: &[String]) -> Result<Self, sqlx::Error> {
        // Keep CARDINALITY($1) > 0 inside NOT EXISTS so empty arrays skip the
        // anti-join via a one-time filter while non-empty arrays still use Hash Anti Join.
        let row = sqlx::query!(
            r#"
            SELECT
                COUNT(*) FILTER (WHERE d.archived = false) AS "total_count!",
                COUNT(*) FILTER (
                    WHERE d.archived = false
                      AND d.last_ping >= now() - INTERVAL '3 minutes'
                ) AS "online_count!",
                COUNT(*) FILTER (
                    WHERE d.archived = false
                      AND d.last_ping < now() - INTERVAL '3 minutes'
                ) AS "offline_count!",
                COUNT(*) FILTER (
                    WHERE d.archived = false
                      AND d.release_id != d.target_release_id
                      AND NOT COALESCE(d.target_release_eligible_at > now(), false)
                ) AS "outdated_count!",
                COUNT(*) FILTER (WHERE d.archived = true) AS "archived_count!"
            FROM device d
            WHERE NOT EXISTS (
                SELECT 1 FROM device_label dl
                JOIN label l ON l.id = dl.label_id
                WHERE CARDINALITY($1::text[]) > 0
                  AND dl.device_id = d.id
                  AND l.name || '=' || dl.value = ANY($1)
            )
            "#,
            excluded_labels
        )
        .fetch_one(pool)
        .await?;

        Ok(Self {
            total_count: u32::try_from(row.total_count).unwrap_or(0),
            online_count: u32::try_from(row.online_count).unwrap_or(0),
            offline_count: u32::try_from(row.offline_count).unwrap_or(0),
            outdated_count: u32::try_from(row.outdated_count).unwrap_or(0),
            archived_count: u32::try_from(row.archived_count).unwrap_or(0),
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct UnhealthyServiceDevice {
    pub device_id: i32,
    pub serial_number: String,
    pub service_name: String,
    pub active_state: String,
    pub n_restarts: i32,
    pub checked_at: DateTime<Utc>,
}

pub async fn get_unhealthy_service_devices(
    pg_pool: &PgPool,
    excluded_labels: &[String],
) -> anyhow::Result<Vec<UnhealthyServiceDevice>> {
    let devices = sqlx::query_as!(
        UnhealthyServiceDevice,
        r#"
        SELECT
            d.id AS device_id,
            d.serial_number,
            rs.service_name,
            dss.active_state,
            dss.n_restarts,
            dss.checked_at
        FROM device d
        JOIN device_service_status dss ON dss.device_id = d.id
        JOIN release_services rs ON rs.id = dss.release_service_id
        WHERE d.last_ping >= now() - INTERVAL '3 minutes'
          AND d.archived = false
          AND (d.release_id = d.target_release_id OR d.target_release_id IS NULL)
          AND rs.release_id = d.release_id
          AND rs.watchdog_sec IS NOT NULL
          AND dss.active_state != 'active'
          AND NOT EXISTS (
              SELECT 1 FROM device_label dl
              JOIN label l ON l.id = dl.label_id
              WHERE CARDINALITY($1::text[]) > 0
                AND dl.device_id = d.id
                AND l.name || '=' || dl.value = ANY($1)
          )
        ORDER BY d.serial_number, rs.service_name
        "#,
        excluded_labels
    )
    .fetch_all(pg_pool)
    .await?;

    Ok(devices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dashboard_new_returns_error_on_closed_pool() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@127.0.0.1:1/postgres")
            .expect("lazy pool");
        pool.close().await;
        assert!(Dashboard::new(&pool, &[]).await.is_err());
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn dashboard_counts_registrations_and_unhealthy_services(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        // Empty table returns all zeros without error for both empty and non-empty excluded labels.
        for excl in [&[][..], &["env=test".to_string()][..]] {
            let empty = Dashboard::new(&pool, excl).await?;
            assert_eq!(
                (
                    empty.total_count,
                    empty.online_count,
                    empty.offline_count,
                    empty.outdated_count,
                    empty.archived_count
                ),
                (0, 0, 0, 0, 0)
            );
            assert!(get_registration_counts(&pool, excl).await?.is_empty());
            assert!(get_unhealthy_service_devices(&pool, excl).await?.is_empty());
        }

        let dist_id: i32 = sqlx::query_scalar(
            "INSERT INTO distribution (name, description, architecture) VALUES ('dash-dist', 'd', 'arm64') RETURNING id",
        )
        .fetch_one(&pool)
        .await?;
        let rel_old: i32 = sqlx::query_scalar(
            "INSERT INTO release (distribution_id, version) VALUES ($1, '1.0.0') RETURNING id",
        )
        .bind(dist_id)
        .fetch_one(&pool)
        .await?;
        let rel_new: i32 = sqlx::query_scalar(
            "INSERT INTO release (distribution_id, version) VALUES ($1, '2.0.0') RETURNING id",
        )
        .bind(dist_id)
        .fetch_one(&pool)
        .await?;

        // Online up-to-date device (with unhealthy watchdog service)
        let d_online_ok: i32 = sqlx::query_scalar(
            "INSERT INTO device (serial_number, approved, archived, last_ping, release_id, target_release_id, created_on)
             VALUES ('dash-1', true, false, NOW() - INTERVAL '30 seconds', $1, $1, '2026-01-15T00:00:00Z') RETURNING id",
        )
        .bind(rel_new)
        .fetch_one(&pool)
        .await?;
        // Offline outdated device (eligible now)
        let _d_offline_outdated: i32 = sqlx::query_scalar(
            "INSERT INTO device (serial_number, approved, archived, last_ping, release_id, target_release_id, target_release_eligible_at, created_on)
             VALUES ('dash-2', true, false, NOW() - INTERVAL '10 minutes', $1, $2, NOW() - INTERVAL '5 minutes', '2026-01-20T00:00:00Z') RETURNING id",
        )
        .bind(rel_old)
        .bind(rel_new)
        .fetch_one(&pool)
        .await?;
        // Online staggered device (eligible in future -> NOT outdated)
        let _d_staggered: i32 = sqlx::query_scalar(
            "INSERT INTO device (serial_number, approved, archived, last_ping, release_id, target_release_id, target_release_eligible_at, created_on)
             VALUES ('dash-3', true, false, NOW() - INTERVAL '10 seconds', $1, $2, NOW() + INTERVAL '1 hour', '2026-02-10T00:00:00Z') RETURNING id",
        )
        .bind(rel_old)
        .bind(rel_new)
        .fetch_one(&pool)
        .await?;
        // Active device with NULL last_ping (counts in total, neither online nor offline)
        let _d_null_ping: i32 = sqlx::query_scalar(
            "INSERT INTO device (serial_number, approved, archived, last_ping, release_id, target_release_id, created_on)
             VALUES ('dash-4', true, false, NULL, $1, $1, '2026-02-11T00:00:00Z') RETURNING id",
        )
        .bind(rel_new)
        .fetch_one(&pool)
        .await?;
        // Archived device
        let _d_archived: i32 = sqlx::query_scalar(
            "INSERT INTO device (serial_number, approved, archived, last_ping, release_id, target_release_id, created_on)
             VALUES ('dash-5', true, true, NOW() - INTERVAL '10 seconds', $1, $2, '2026-02-12T00:00:00Z') RETURNING id",
        )
        .bind(rel_old)
        .bind(rel_new)
        .fetch_one(&pool)
        .await?;
        // Online excluded-label device (with unhealthy watchdog service)
        let d_excluded: i32 = sqlx::query_scalar(
            "INSERT INTO device (serial_number, approved, archived, last_ping, release_id, target_release_id, created_on)
             VALUES ('dash-6', true, false, NOW() - INTERVAL '15 seconds', $1, $1, '2026-02-13T00:00:00Z') RETURNING id",
        )
        .bind(rel_new)
        .fetch_one(&pool)
        .await?;

        let label_id: i32 =
            sqlx::query_scalar("INSERT INTO label (name) VALUES ('env') RETURNING id")
                .fetch_one(&pool)
                .await?;
        sqlx::query(
            "INSERT INTO device_label (device_id, label_id, value) VALUES ($1, $2, 'test')",
        )
        .bind(d_excluded)
        .bind(label_id)
        .execute(&pool)
        .await?;

        // Services on rel_new: two watchdog services and one non-watchdog service.
        let svc_watchdog: i32 = sqlx::query_scalar(
            "INSERT INTO release_services (release_id, service_name, watchdog_sec) VALUES ($1, 'smithd.service', 30) RETURNING id",
        )
        .bind(rel_new)
        .fetch_one(&pool)
        .await?;
        let svc_watchdog_2: i32 = sqlx::query_scalar(
            "INSERT INTO release_services (release_id, service_name, watchdog_sec) VALUES ($1, 'brain.service', 60) RETURNING id",
        )
        .bind(rel_new)
        .fetch_one(&pool)
        .await?;
        let svc_no_watchdog: i32 = sqlx::query_scalar(
            "INSERT INTO release_services (release_id, service_name, watchdog_sec) VALUES ($1, 'helper.service', NULL) RETURNING id",
        )
        .bind(rel_new)
        .fetch_one(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO device_service_status (device_id, release_service_id, active_state, n_restarts, checked_at)
             VALUES ($1, $3, 'failed', 2, NOW()), ($1, $4, 'inactive', 0, NOW()), ($1, $5, 'failed', 1, NOW()), ($2, $3, 'failed', 3, NOW())",
        )
        .bind(d_online_ok)
        .bind(d_excluded)
        .bind(svc_watchdog)
        .bind(svc_watchdog_2)
        .bind(svc_no_watchdog)
        .execute(&pool)
        .await?;

        // Without exclusions
        let all = Dashboard::new(&pool, &[]).await?;
        assert_eq!(all.total_count, 5);
        assert_eq!(all.online_count, 3);
        assert_eq!(all.offline_count, 1);
        assert_eq!(all.outdated_count, 1);
        assert_eq!(all.archived_count, 1);

        let regs_all = get_registration_counts(&pool, &[]).await?;
        assert_eq!(regs_all.len(), 2);
        assert_eq!((regs_all[0].count, regs_all[1].count), (2, 4));

        let unhealthy_all = get_unhealthy_service_devices(&pool, &[]).await?;
        assert_eq!(
            unhealthy_all
                .iter()
                .map(|r| (r.serial_number.as_str(), r.service_name.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("dash-1", "brain.service"),
                ("dash-1", "smithd.service"),
                ("dash-6", "smithd.service"),
            ]
        );

        // With env=test excluded
        let excluded = vec!["env=test".to_string()];
        let filtered = Dashboard::new(&pool, &excluded).await?;
        assert_eq!(filtered.total_count, 4);
        assert_eq!(filtered.online_count, 2);
        assert_eq!(filtered.offline_count, 1);
        assert_eq!(filtered.outdated_count, 1);
        assert_eq!(filtered.archived_count, 1);

        let regs_filtered = get_registration_counts(&pool, &excluded).await?;
        assert_eq!((regs_filtered[0].count, regs_filtered[1].count), (2, 3));

        let unhealthy_filtered = get_unhealthy_service_devices(&pool, &excluded).await?;
        assert_eq!(
            unhealthy_filtered
                .iter()
                .map(|r| (r.serial_number.as_str(), r.service_name.as_str()))
                .collect::<Vec<_>>(),
            vec![("dash-1", "brain.service"), ("dash-1", "smithd.service")]
        );
        Ok(())
    }
}
