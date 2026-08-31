use models::release::Release;

pub mod route;

pub async fn get_release_by_id(
    release_id: i32,
    pg_pool: &sqlx::PgPool,
) -> Result<Option<Release>, sqlx::Error> {
    sqlx::query_as!(
        Release,
        "
        SELECT release.*,
        distribution.name AS distribution_name,
        distribution.architecture AS distribution_architecture,
        auth.users.email AS user_email
        FROM release
        JOIN distribution ON release.distribution_id = distribution.id
        LEFT JOIN auth.users ON release.user_id = auth.users.id
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
) -> Result<Option<Release>, sqlx::Error> {
    sqlx::query_as!(
        Release,
        "
        SELECT release.*,
        distribution.name AS distribution_name,
        distribution.architecture AS distribution_architecture,
        auth.users.email AS user_email
        FROM release
        JOIN distribution ON release.distribution_id = distribution.id
            AND distribution.latest_release_id = release.id
        LEFT JOIN auth.users ON release.user_id = auth.users.id
        WHERE distribution_id = $1
        ",
        distribution_id
    )
    .fetch_optional(pg_pool)
    .await
}

/// The release a device should be flashed with: the most recently designated
/// LTS release for the distribution that is still published and not withdrawn.
/// Earlier LTS releases keep their flag, so the history of what was designated
/// stays intact -- this only picks the one currently in force.
pub async fn get_current_lts_release(
    distribution_id: i32,
    pg_pool: &sqlx::PgPool,
) -> Result<Option<Release>, sqlx::Error> {
    sqlx::query_as!(
        Release,
        "
        SELECT release.*,
        distribution.name AS distribution_name,
        distribution.architecture AS distribution_architecture,
        auth.users.email AS user_email
        FROM release
        JOIN distribution ON release.distribution_id = distribution.id
        LEFT JOIN auth.users ON release.user_id = auth.users.id
        WHERE release.distribution_id = $1
            AND release.lts
            AND NOT release.draft
            AND NOT release.yanked
        ORDER BY release.lts_marked_at DESC NULLS LAST, release.id DESC
        LIMIT 1
        ",
        distribution_id
    )
    .fetch_optional(pg_pool)
    .await
}
