use crate::State;
use axum::http::StatusCode;
use axum::response::Result;
use axum::{
    Extension, Json,
    extract::{Path, Query},
};
use smith::utils::schema::NetworkType;
use smith::utils::schema::{Network, NewNetwork};
use tracing::error;

const NETWORKS_TAG: &str = "networks";

#[derive(Debug, serde::Deserialize)]
pub struct SerialNumbers {
    serial_numbers: Option<String>,
}

#[utoipa::path(
    get,
    path = "/networks",
    params(
        ("serial_numbers" = Option<String>, Query, description = "Optional list of device serial numbers to filter networks. If not provided, returns all networks")
    ),
    responses(
        (status = 200, description = "List of networks retrieved successfully"),
        (status = 500, description = "Failed to retrieve networks", body = String),
    ),
    security(("Access Token" = [])),
    tag = NETWORKS_TAG
)]
pub async fn get_networks(
    Extension(state): Extension<State>,
    Query(query): Query<SerialNumbers>,
) -> Result<Json<Vec<Network>>, StatusCode> {
    let networks = match query.serial_numbers {
        Some(serial_numbers) => {
            let serials: Vec<String> = serial_numbers.split(',').map(String::from).collect();
            sqlx::query_as!(
                Network,
                r#"
                SELECT
                    n.id,
                    n.network_type::TEXT as "network_type",
                    n.is_network_hidden,
                    n.ssid,
                    n.name,
                    n.description,
                    n.password
                FROM network n
                JOIN device d ON n.id = d.network_id
                WHERE d.serial_number = ANY($1)
                "#,
                &serials[..]
            )
            .fetch_all(&state.pg_pool)
            .await
        }
        None => {
            sqlx::query_as!(
                Network,
                r#"
                SELECT
                    n.id,
                    n.network_type::TEXT as "network_type",
                    n.is_network_hidden,
                    n.ssid,
                    n.name,
                    n.description,
                    n.password
                FROM network n
                "#
            )
            .fetch_all(&state.pg_pool)
            .await
        }
    }
    .map_err(|err| {
        error!("error: failed to get networks: {:?}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(networks))
}

#[utoipa::path(
    get,
    path = "/networks/{network_id}",
    responses(
        (status = 200, description = "Return found network"),
        (status = 500, description = "Failed to retrieve network", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = NETWORKS_TAG
)]
pub async fn get_network_by_id(
    Path(network_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Json<Network>, StatusCode> {
    let network = sqlx::query_as!(
        Network,
        r#"
        SELECT
            network.id,
            network.network_type::TEXT,
            network.is_network_hidden,
            network.ssid,
            network.name,
            network.description,
            network.password
        FROM network
        WHERE network.id = $1
        "#,
        network_id
    )
    .fetch_one(&state.pg_pool)
    .await
    .map_err(|err| {
        error!(
            "error: failed to get network for id {}: {:?}",
            network_id, err
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(network))
}

#[utoipa::path(
    delete,
    path = "/networks/{network_id}",
    responses(
        (status = StatusCode::NO_CONTENT, description = "Successfully deleted the network"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to delete network", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = NETWORKS_TAG
)]
pub async fn delete_network_by_id(
    Path(network_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<StatusCode, StatusCode> {
    sqlx::query!(r#"DELETE FROM network WHERE id = $1"#, network_id)
        .execute(&state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to delete network {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/networks",
    responses(
        (status = 201, description = "Network created successfully"),
        (status = 304, description = "Network was not modified"),
        (status = 500, description = "Failed to create network", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = NETWORKS_TAG
)]
pub async fn create_network(
    Extension(state): Extension<State>,
    Json(new_network): Json<NewNetwork>,
) -> Result<(StatusCode, Json<Network>), StatusCode> {
    let created_network = sqlx::query_as!(
        Network,
        r#"
        INSERT INTO network (network_type, is_network_hidden, ssid, name, description, password)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, network_type::TEXT as "network_type", is_network_hidden, ssid, name, description, NULL as password
        "#,
        new_network.network_type as NetworkType,
        new_network.is_network_hidden,
        new_network.ssid,
        new_network.name,
        new_network.description,
        new_network.password,
    )
    .fetch_one(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to insert network {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok((StatusCode::CREATED, Json(created_network)))
}
