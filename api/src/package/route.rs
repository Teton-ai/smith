use crate::State;
use crate::package::Package;
use axum::http::StatusCode;
use axum::{Extension, Json};
use axum_typed_multipart::{FieldData, TryFromMultipart, TypedMultipart};
use smith::utils::schema;
use std::io::{Cursor, Read};
use tempfile::NamedTempFile;
use tracing::{debug, error};

const PACKAGES_TAG: &str = "packages";

/// Retrieve all registered packages ordered by creation time.
///
/// # Returns
///
/// A `Json<Vec<schema::Package>>` containing the list of packages on success; returns HTTP `500` on failure.
///
/// # Examples
///
/// ```
/// use axum::response::Json;
///
/// // Construct a JSON value representing packages for documentation purposes.
/// let packages: Vec<schema::Package> = Vec::new();
/// let _json: Json<Vec<schema::Package>> = Json(packages);
/// ```
#[utoipa::path(
get,
path = "/packages",
responses(
(status = 200, description = "List of registered packages"),
(status = 500, description = "Failure", body = String),
),
security(
("auth_token" = [])
),
tag = PACKAGES_TAG
)]
pub async fn get_packages(
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<Vec<schema::Package>>, StatusCode> {
    let packages = sqlx::query_as!(
        schema::Package,
        "SELECT * FROM package ORDER BY package.created_at DESC"
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get packages {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(packages))
}

#[derive(Debug, TryFromMultipart, utoipa::ToSchema)]
pub struct ReleasePackageRequest {
    #[schema(format = Binary, value_type = String)]
    #[form_data(limit = "1Gib")]
    file: FieldData<NamedTempFile>,
}

/// Handle an incoming multipart package upload, parse the uploaded Debian package, and persist it.
///
/// This endpoint accepts a multipart/form-data request containing a Debian package file, extracts
/// the package control metadata (name, version, architecture), and stores the package using the
/// application's Package::new routine. On success it responds with HTTP 200; on failure it
/// returns HTTP 500.
///
/// # Examples
///
/// ```no_run
/// use axum::http::StatusCode;
/// // `state` and `multipart` are placeholders representing the extension State and parsed multipart.
/// // In an integration test you would construct an HTTP request with multipart/form-data and invoke the handler.
/// # async fn example(state: crate::State, multipart: crate::ReleasePackageRequest) {
/// let result: axum::response::Result<StatusCode, StatusCode> =
///     crate::handlers::release_package(axum::Extension(state), axum_extra::extract::TypedMultipart(multipart)).await;
/// match result {
///     Ok(StatusCode::OK) => println!("uploaded"),
///     Err(StatusCode::INTERNAL_SERVER_ERROR) => println!("failed"),
///     _ => println!("other"),
/// }
/// # }
/// ```
#[utoipa::path(
put,
path = "/packages",
request_body(content = ReleasePackageRequest, content_type = "multipart/form-data"),
responses(
(status = 200, description = "Sucess releasing package"),
(status = 500, description = "Failure", body = String),
),
security(
("auth_token" = [])
),
tag = PACKAGES_TAG
)]
pub async fn release_package(
    Extension(state): Extension<State>,
    TypedMultipart(ReleasePackageRequest { mut file }): TypedMultipart<ReleasePackageRequest>,
) -> axum::response::Result<StatusCode, StatusCode> {
    let file_name = file.metadata.file_name.unwrap_or(String::from("data.bin"));

    let mut buf = Vec::new();
    file.contents.read_to_end(&mut buf).map_err(|err| {
        error!("error: failed to release package {:?}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut cursor = Cursor::new(&buf);
    let mut pkg = debpkg::DebPkg::parse(&mut cursor).unwrap();

    let control_tar = pkg.control().unwrap();
    let control = debpkg::Control::extract(control_tar).unwrap();
    let arch = control.get("Architecture").unwrap();
    debug!("File Name: {}", file_name);
    debug!("Package Name: {}", control.name());
    debug!("Package Version: {}", control.version());
    debug!("Package Architecture: {}", arch);

    Package::new(
        control.name(),
        control.version(),
        arch,
        &file_name,
        &buf,
        state.config,
        &state.pg_pool,
    )
    .await
    .map_err(|err| {
        error!("error: Failed to save package: {:?}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(StatusCode::OK)
}