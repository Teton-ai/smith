use crate::event::PublicEvent;
use axum::error_handling::HandleErrorLayer;
use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::response::Redirect;
use axum::{Extension, Router, middleware, routing::get};
use config::Config;
use middlewares::authorization::AuthorizationConfig;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::borrow::Cow;
use std::env;
use std::fs::File;
use std::future::ready;
use std::io::Read;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::broadcast::Sender;
use tokio::sync::{Mutex, broadcast};
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tower_http::decompression::RequestDecompressionLayer;
use tracing::info;
use tracing_subscriber::{EnvFilter, prelude::*};
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa_swagger_ui::SwaggerUi;

mod asset;
mod auth;
mod command;
mod config;
mod dashboard;
mod db;
mod deployment;
mod device;
mod event;
mod handlers;
pub mod health;
pub mod ip_address;
mod metric;
mod middlewares;
mod modem;
mod package;
mod rollout;
mod smith;
mod storage;
mod telemetry;
mod user;

#[derive(Clone, Debug)]
pub struct State {
    pg_pool: PgPool,
    config: &'static Config,
    public_events: Arc<Mutex<Sender<PublicEvent>>>,
    authorization: Arc<AuthorizationConfig>,
}

fn main() {
    let roles_path =
        env::var("ROLES_PATH").unwrap_or_else(|_| "/workspace/api/roles.toml".to_string());

    let mut roles = File::open(&roles_path)
        .unwrap_or_else(|_| panic!("Failed to open roles file at {}", roles_path));

    let mut roles_toml = String::new();
    roles
        .read_to_string(&mut roles_toml)
        .expect("Failed to read roles file");

    let authorization =
        AuthorizationConfig::new(&roles_toml).expect("Failed to load authorization config");

    let config: &'static Config = Box::leak(Box::new(
        Config::new().expect("error: failed to construct config"),
    ));

    if let Some(sentry_url) = &config.sentry_url {
        // Sentry needs to be initialized outside of an async block.
        // See https://docs.sentry.io/platforms/rust.
        let _guard = sentry::init(sentry::ClientOptions {
            dsn: Some(sentry_url.parse().expect("Invalid Sentry DSN")),
            traces_sample_rate: 0.75,
            release: sentry::release_name!(),
            environment: match env::var("ENVIRONMENT") {
                Ok(value) => Some(Cow::Owned(value)),
                Err(_) => Some(Cow::Borrowed("development")),
            },
            ..Default::default()
        });
    }

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(sentry_tracing::layer())
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    // Corresponds to `#[tokio::main]`.
    // See https://docs.rs/tokio-macros/latest/src/tokio_macros/lib.rs.html#225.
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("error: failed to initialize tokio runtime")
        .block_on(async {
            _ = tokio::spawn(async move { start_main_server(config, authorization).await }).await;
        });
}

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "Access Token",
                SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("Authorization"))),
            )
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    modifiers(&SecurityAddon),
    info(
        title = "API",
        contact(name = "Smith Security Team", email = "security@teton.ai")
    )
)]
struct ApiDoc;

#[derive(OpenApi)]
#[openapi(
    modifiers(&SecurityAddon),
    info(
        title = "Smith API",
        contact(name = "Smith Security Team", email = "security@teton.ai")
    )
)]
struct SmithApiDoc;

async fn start_main_server(config: &'static Config, authorization: AuthorizationConfig) {
    info!("Starting Smith API v{}", env!("CARGO_PKG_VERSION"));
    // set up connection pool
    let pool = PgPoolOptions::new()
        .max_connections(100)
        .min_connections(10)
        .connect(&config.database_url)
        .await
        .expect("can't connect to database.");

    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("sqlx migration failed");

    let (tx_message, _rx_message) = broadcast::channel::<PublicEvent>(1);
    let tx_message = Arc::new(Mutex::new(tx_message));

    let state = State {
        pg_pool: pool,
        config,
        public_events: tx_message,
        authorization: Arc::new(authorization),
    };

    let recorder_handle = metric::setup_metrics_recorder();

    let mut api_doc = ApiDoc::openapi();

    let (public_router, public_api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(command::route::available_commands))
        .split_for_parts();
    api_doc.merge(public_api);

    #[allow(deprecated)]
    let (protected_router, protected_api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(dashboard::route::api))
        .routes(routes!(auth::route::verify_token))
        .routes(routes!(
            handlers::network::get_networks,
            handlers::network::create_network
        ))
        .routes(routes!(
            handlers::network::get_network_by_id,
            handlers::network::delete_network_by_id
        ))
        .routes(routes!(handlers::devices::get_devices))
        .routes(routes!(
            handlers::devices::get_device_info,
            handlers::devices::delete_device,
            handlers::devices::update_device
        ))
        .routes(routes!(handlers::devices::get_health_for_device))
        .routes(routes!(
            package::route::get_packages,
            package::route::release_package
        ))
        .routes(routes!(modem::route::get_modem_list))
        .routes(routes!(modem::route::get_modem_by_id))
        .routes(routes!(
            handlers::distributions::get_distributions,
            handlers::distributions::create_distribution
        ))
        .routes(routes!(
            handlers::distributions::get_distribution_by_id,
            handlers::distributions::delete_distribution_by_id
        ))
        .routes(routes!(
            handlers::distributions::get_distribution_releases,
            handlers::distributions::create_distribution_release,
        ))
        .routes(routes!(handlers::distributions::get_distribution_devices))
        .routes(routes!(
            handlers::distributions::get_distribution_latest_release
        ))
        .routes(routes!(ip_address::route::get_ip_addresses,))
        .routes(routes!(
            ip_address::route::get_ip_address_info,
            ip_address::route::update_ip_address
        ))
        .routes(routes!(handlers::releases::get_releases))
        .routes(routes!(
            handlers::releases::get_release,
            handlers::releases::update_release
        ))
        .routes(routes!(
            handlers::releases::get_distribution_release_packages,
            handlers::releases::add_package_to_release
        ))
        .routes(routes!(
            handlers::releases::update_package_for_release,
            handlers::releases::delete_package_for_release
        ))
        .routes(routes!(
            handlers::devices::get_network_for_device,
            handlers::devices::update_device_network
        ))
        .routes(routes!(handlers::devices::update_devices_network))
        .routes(routes!(
            handlers::devices::issue_commands_to_device,
            handlers::devices::get_all_commands_for_device
        ))
        .routes(routes!(rollout::route::api_rollout,))
        .routes(routes!(deployment::route::api_get_deployment_devices))
        .routes(routes!(
            deployment::route::api_release_deployment,
            deployment::route::api_get_release_deployment,
            deployment::route::api_release_deployment_check_done
        ))
        .nest_service(
            "/packages/:package_id",
            get(handlers::packages::get_package_by_id)
                .delete(handlers::packages::delete_package_by_id),
        )
        .routes(routes!(handlers::devices::get_tag_for_device))
        .routes(routes!(
            handlers::devices::delete_tag_from_device,
            handlers::devices::add_tag_to_device
        ))
        .routes(routes!(
            handlers::devices::get_variables_for_device,
            handlers::devices::add_variable_to_device
        ))
        .routes(routes!(
            handlers::devices::delete_variable_from_device,
            handlers::devices::update_variable_for_device
        ))
        .routes(routes!(handlers::devices::update_note_for_device))
        .routes(routes!(
            handlers::devices::get_device_release,
            handlers::devices::update_device_target_release
        ))
        .routes(routes!(handlers::devices::get_ledger_for_device))
        .routes(routes!(
            handlers::devices::approve_device,
            handlers::devices::revoke_device
        ))
        .routes(routes!(handlers::devices::delete_token))
        .routes(routes!(handlers::devices::get_tags))
        .routes(routes!(handlers::devices::update_devices_target_release))
        .routes(routes!(handlers::devices::get_variables))
        .routes(routes!(
            handlers::tags::get_tags,
            handlers::tags::create_tag
        ))
        .routes(routes!(
            command::route::get_bundle_commands,
            command::route::issue_commands_to_devices
        ))
        .routes(routes!(handlers::devices::get_devices_new))
        .routes(routes!(event::route::sse_handler))
        .route_layer(middleware::from_fn(middlewares::authentication::check))
        // TODO: Check why we have this, not good for all routes
        .layer(DefaultBodyLimit::max(891289600))
        .split_for_parts();
    api_doc.merge(protected_api);

    let (smith_router, smith_api) = OpenApiRouter::with_openapi(SmithApiDoc::openapi())
        .routes(routes!(smith::route::register_device))
        .routes(routes!(smith::route::home))
        .routes(routes!(telemetry::route::modem))
        .routes(routes!(telemetry::route::victoria))
        .routes(routes!(smith::route::upload_file))
        .routes(routes!(smith::route::download_file))
        .routes(routes!(smith::route::fetch_package))
        .routes(routes!(smith::route::list_release_packages))
        .routes(routes!(smith::route::test_file))
        .split_for_parts();

    let smith_router = smith_router
        .layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(|_| async move {
                    (StatusCode::INTERNAL_SERVER_ERROR, "Unhandled server error")
                }))
                .layer(RequestDecompressionLayer::new()),
        )
        .layer(DefaultBodyLimit::max(512000000));

    let app = Router::new()
        .route("/", get(|| async { Redirect::temporary("/docs") }))
        .merge(public_router)
        .merge(protected_router)
        .merge(smith_router)
        .route("/metrics", get(move || ready(recorder_handle.render())))
        .route("/health", get(health::check))
        .merge(SwaggerUi::new("/docs").url("/openapi.json", api_doc))
        .merge(SwaggerUi::new("/smith/docs").url("/smith/openapi.json", smith_api))
        .layer(CorsLayer::permissive())
        .route_layer(middleware::from_fn(metric::track_metrics))
        .layer(Extension(state));

    let listener = TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("error: failed to bind to port");
    info!(
        "Smith API running on http://{} (Press Ctrl+C to quit)",
        listener.local_addr().unwrap().to_string()
    );
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .expect("error: failed to initialize axum server");
}
