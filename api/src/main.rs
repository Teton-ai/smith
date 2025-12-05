use crate::auth::DebugJwksClient;
use crate::event::PublicEvent;
use crate::sentry::Sentry;
use ::sentry::integrations::tower::{NewSentryLayer, SentryHttpLayer};
use axum::body::Body;
use axum::error_handling::HandleErrorLayer;
use axum::extract::{DefaultBodyLimit, MatchedPath};
use axum::http::{Request, StatusCode};
use axum::response::Redirect;
use axum::{Extension, Router, middleware, routing::get};
use config::Config;
use middlewares::authorization::AuthorizationConfig;
use sqlx::postgres::{PgPool, PgPoolOptions};
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
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::{EnvFilter, prelude::*};
use utoipa::openapi::security::{Http, HttpAuthScheme, SecurityScheme};
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
mod distribution;
mod event;
mod handlers;
mod health;
mod ip_address;
mod metric;
mod middlewares;
mod modem;
pub mod network;
mod package;
mod release;
mod rollout;
mod sentry;
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
    jwks_client: DebugJwksClient,
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

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_line_number(true)
                .compact(),
        )
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let _sentry_guard = Sentry::init(config);

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("error: failed to initialize tokio runtime")
        .block_on(async { start_main_server(config, authorization).await });
}

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "auth_token",
                SecurityScheme::Http({
                    let mut http = Http::new(HttpAuthScheme::Bearer);
                    http.description = Some("Auth Token for authentication".to_string());
                    http
                }),
            );
            components.add_security_scheme(
                "device_token",
                SecurityScheme::Http({
                    let mut http = Http::new(HttpAuthScheme::Bearer);
                    http.description = Some("Device token for authentication".to_string());
                    http
                }),
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

    // Create JwksClient once at startup
    let jwks_client =
        DebugJwksClient::init(&config.auth0_issuer).expect("Failed to initialize JWKS client");

    let state = State {
        pg_pool: pool,
        config,
        public_events: tx_message,
        authorization: Arc::new(authorization),
        jwks_client,
    };

    let recorder_handle = metric::setup_metrics_recorder();

    let mut api_doc = ApiDoc::openapi();

    let (device_router, device_api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(device::route::get_device))
        .route_layer(middleware::from_fn(device::Device::middleware))
        .split_for_parts();
    api_doc.merge(device_api);

    let (public_router, public_api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(command::route::available_commands))
        .split_for_parts();

    api_doc.merge(public_api);

    #[allow(deprecated)]
    let (protected_router, protected_api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(dashboard::route::api))
        .routes(routes!(auth::route::verify_token))
        .routes(routes!(
            network::route::get_networks,
            network::route::create_network
        ))
        .routes(routes!(
            network::route::get_network_by_id,
            network::route::delete_network_by_id
        ))
        .routes(routes!(device::route::get_devices))
        .routes(routes!(
            device::route::get_device_info,
            device::route::delete_device,
            device::route::update_device
        ))
        .routes(routes!(device::route::delete_label))
        .routes(routes!(device::route::get_health_for_device))
        .routes(routes!(
            package::route::get_packages,
            package::route::release_package
        ))
        .routes(routes!(modem::route::get_modem_list))
        .routes(routes!(modem::route::get_modem_by_id))
        .routes(routes!(
            distribution::route::get_distributions,
            distribution::route::create_distribution
        ))
        .routes(routes!(
            distribution::route::get_distribution_by_id,
            distribution::route::delete_distribution_by_id
        ))
        .routes(routes!(
            distribution::route::get_distribution_releases,
            distribution::route::create_distribution_release,
        ))
        .routes(routes!(distribution::route::get_distribution_devices))
        .routes(routes!(
            distribution::route::get_distribution_latest_release
        ))
        .routes(routes!(ip_address::route::get_ip_addresses,))
        .routes(routes!(
            ip_address::route::get_ip_address_info,
            ip_address::route::update_ip_address
        ))
        .routes(routes!(release::route::get_releases))
        .routes(routes!(
            release::route::get_release,
            release::route::update_release
        ))
        .routes(routes!(
            release::route::get_distribution_release_packages,
            release::route::add_package_to_release
        ))
        .routes(routes!(
            release::route::update_package_for_release,
            release::route::delete_package_for_release
        ))
        .routes(routes!(
            device::route::get_network_for_device,
            device::route::update_device_network
        ))
        .routes(routes!(device::route::update_devices_network))
        .routes(routes!(
            device::route::issue_commands_to_device,
            device::route::get_all_commands_for_device
        ))
        .routes(routes!(rollout::route::api_rollout,))
        .routes(routes!(deployment::route::api_get_deployment_devices))
        .routes(routes!(
            deployment::route::api_release_deployment,
            deployment::route::api_get_release_deployment,
            deployment::route::api_release_deployment_check_done
        ))
        .routes(routes!(deployment::route::api_confirm_full_rollout))
        .nest_service(
            "/packages/:package_id",
            get(handlers::packages::get_package_by_id)
                .delete(handlers::packages::delete_package_by_id),
        )
        .routes(routes!(device::route::get_tag_for_device))
        .routes(routes!(
            device::route::delete_tag_from_device,
            device::route::add_tag_to_device
        ))
        .routes(routes!(
            device::route::get_variables_for_device,
            device::route::add_variable_to_device
        ))
        .routes(routes!(
            device::route::delete_variable_from_device,
            device::route::update_variable_for_device
        ))
        .routes(routes!(device::route::update_note_for_device))
        .routes(routes!(
            device::route::get_device_release,
            device::route::update_device_target_release
        ))
        .routes(routes!(device::route::get_ledger_for_device))
        .routes(routes!(
            device::route::approve_device,
            device::route::revoke_device
        ))
        .routes(routes!(device::route::delete_token))
        .routes(routes!(device::route::get_tags))
        .routes(routes!(device::route::update_devices_target_release))
        .routes(routes!(device::route::get_variables))
        .routes(routes!(
            handlers::tags::get_tags,
            handlers::tags::create_tag
        ))
        .routes(routes!(
            command::route::get_bundle_commands,
            command::route::issue_commands_to_devices
        ))
        .routes(routes!(device::route::get_devices_new))
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
        .routes(routes!(smith::route::test_upload))
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
        .merge(device_router)
        .merge(protected_router)
        .merge(smith_router)
        .route("/metrics", get(move || ready(recorder_handle.render())))
        .route("/health", get(health::check))
        .merge(SwaggerUi::new("/docs").url("/openapi.json", api_doc))
        .merge(SwaggerUi::new("/smith/docs").url("/smith/openapi.json", smith_api))
        .layer(CorsLayer::permissive())
        .route_layer(middleware::from_fn(metric::track_metrics))
        .layer(middleware::from_fn(Sentry::capture_errors_middleware))
        .layer(
            ServiceBuilder::new()
                .layer(NewSentryLayer::<Request<Body>>::new_from_top())
                .layer(SentryHttpLayer::new().enable_transaction()),
        )
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &Request<Body>| {
                let path = if let Some(path) = request.extensions().get::<MatchedPath>() {
                    path.as_str()
                } else {
                    request.uri().path()
                };
                tracing::info_span!(
                    "http-request",
                    "http.request.method" = %request.method(),
                    "http.route" = path
                )
            }),
        )
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
