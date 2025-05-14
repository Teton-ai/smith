use crate::State;
use crate::modem::schema::Modem;
use axum::{Extension, Json, extract::Path};
use axum::{http::StatusCode, response::Result};
use tracing::error;

const TAG: &str = "modems";

#[utoipa::path(
    get,
    path = "/modems",
    responses(
        (status = StatusCode::OK, description = "List of modems retrieved successfully", body = Vec<Modem>),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve modems"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = TAG
)]
pub async fn get_modem_list(
    Extension(state): Extension<State>,
) -> Result<Json<Vec<Modem>>, StatusCode> {
    let modems = sqlx::query_as!(Modem, "SELECT * FROM modem")
        .fetch_all(&state.pg_pool)
        .await
        .map_err(|err| {
            error!("error: failed to get modems: {:?}", err);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(modems))
}

#[utoipa::path(
    get,
    path = "/modems/:modem_id",
    responses(
        (status = StatusCode::OK, description = "Return found modem", body = Modem),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve modem"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = TAG
)]
pub async fn get_modem_by_id(
    Path(modem_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Json<Modem>, StatusCode> {
    let modem = sqlx::query_as!(Modem, "SELECT * FROM modem WHERE id = $1", modem_id)
        .fetch_one(&state.pg_pool)
        .await
        .map_err(|err| {
            error!("error: failed to get modem for id {}: {:?}", modem_id, err);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(modem))
}
