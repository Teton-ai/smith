use crate::release::Release;

pub async fn db_get_release_by_id(
    release_id: i32,
    pg_pool: &sqlx::PgPool,
) -> Result<Option<Release>, sqlx::Error> {
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

pub async fn db_get_latest_distribution_release(
    distribution_id: i32,
    pg_pool: &sqlx::PgPool,
) -> Result<Release, sqlx::Error> {
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
