use axum::Router;
use axum::error_handling::HandleErrorLayer;
use axum::extract::{DefaultBodyLimit, MatchedPath};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::IntoResponse;
use axum::{
    Extension,
    extract::Request,
    middleware,
    routing::{any, get, post},
};
use config::Config;
use handlers::events::PublicEvent;
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use middlewares::authorization::AuthorizationConfig;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::borrow::Cow;
use std::env;
use std::fs::File;
use std::future::ready;
use std::io::Read;
use std::sync::Arc;
use std::time::Instant;
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
use utoipa_scalar::{Scalar, Servable as ScalarServable};

mod asset;
mod config;
mod dashboard;
mod db;
mod deployment;
mod device;
mod handlers;
mod middlewares;
mod modem;
mod package;
mod rollout;
mod storage;
mod telemetry;
mod users;

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
#[openapi(modifiers(&SecurityAddon))]
struct ApiDoc;

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

    let recorder_handle = setup_metrics_recorder();

    // build our application with a route
    let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(dashboard::api))
        .routes(routes!(handlers::auth::verify_token))
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
            handlers::devices::delete_device
        ))
        .routes(routes!(handlers::devices::get_health_for_device))
        .routes(routes!(
            handlers::packages::get_packages,
            handlers::packages::release_package
        ))
        .routes(routes!(modem::routes::get_modem_list))
        .routes(routes!(modem::routes::get_modem_by_id))
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
        .routes(routes!(rollout::routes::api_rollout,))
        .routes(routes!(
            deployment::routes::api_release_deployment,
            deployment::routes::api_get_release_deployment,
            deployment::routes::api_release_deployment_check_done
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
        .routes(routes!(handlers::commands::get_commands))
        .routes(routes!(
            handlers::commands::get_bundle_commands,
            handlers::commands::issue_commands_to_devices
        ))
        .routes(routes!(handlers::devices::get_devices_new))
        // Auth middleware. Every route prior to this is protected.
        .route_layer(middleware::from_fn(middlewares::authentication::check))
        .routes(routes!(handlers::events::sse_handler))
        .layer(DefaultBodyLimit::max(891289600))
        .split_for_parts();

    // !Routes after the auth layer are not protected!
    let smith_router = Router::new()
        .route(
            "/smith/register",
            post(handlers::home::register_device).layer(
                ServiceBuilder::new()
                    .layer(HandleErrorLayer::new(|_| async move {
                        (StatusCode::INTERNAL_SERVER_ERROR, "Unhandled server error")
                    }))
                    .layer(RequestDecompressionLayer::new()),
            ),
        )
        .route(
            "/smith/home",
            post(handlers::home::home).layer(
                ServiceBuilder::new()
                    .layer(HandleErrorLayer::new(|_| async move {
                        (StatusCode::INTERNAL_SERVER_ERROR, "Unhandled server error")
                    }))
                    .layer(RequestDecompressionLayer::new()),
            ),
        )
        .route(
            "/smith/telemetry/modem",
            post(telemetry::routes::modem).layer(
                ServiceBuilder::new()
                    .layer(HandleErrorLayer::new(|_| async move {
                        (StatusCode::INTERNAL_SERVER_ERROR, "Unhandled server error")
                    }))
                    .layer(RequestDecompressionLayer::new()),
            ),
        )
        .route(
            "/smith/telemetry/victoria",
            any(telemetry::routes::victoria),
        )
        .route(
            "/smith/upload",
            post(handlers::upload::upload_file).layer(DefaultBodyLimit::max(512000000)),
        )
        .route(
            "/smith/upload/*path",
            post(handlers::upload::upload_file).layer(DefaultBodyLimit::max(512000000)),
        )
        .route("/smith/download", get(handlers::download::download_file))
        .route(
            "/smith/download/*path",
            get(handlers::download::download_file),
        )
        .nest_service("/smith/package", get(handlers::fetch_package))
        .route(
            "/smith/releases/:release_id/packages",
            get(handlers::list_release_packages),
        );

    let json_specification = api.to_pretty_json().expect("API docs generation failed");

    let app = router
        .merge(smith_router)
        .route("/metrics", get(move || ready(recorder_handle.render())))
        .route("/health", get(handlers::health::check))
        .route_layer(middleware::from_fn(track_metrics))
        .layer(Extension(state))
        .route(
            "/docs/openapi.json",
            get(move || ready(json_specification.clone())),
        )
        .layer(CorsLayer::permissive())
        .merge(Scalar::with_url("/docs", api));

    let listener = TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("error: failed to bind to port");
    info!(
        "Smith API running on http://{} (Press Ctrl+C to quit)",
        listener.local_addr().unwrap().to_string()
    );
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .await
        .expect("error: failed to initialize axum server");
}

fn setup_metrics_recorder() -> PrometheusHandle {
    // Metrics
    const EXPONENTIAL_SECONDS: &[f64] = &[
        0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
    ];

    PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full("http_requests_duration_seconds".to_string()),
            EXPONENTIAL_SECONDS,
        )
        .expect("error: failed to build prometheus recorder")
        .install_recorder()
        .expect("error: failed to install prometheus recorder")
}

async fn track_metrics(req: Request, next: Next) -> impl IntoResponse {
    let start = Instant::now();
    let path = if let Some(matched_path) = req.extensions().get::<MatchedPath>() {
        matched_path.as_str().to_owned()
    } else {
        req.uri().path().to_owned()
    };
    let method = req.method().clone();

    let response = next.run(req).await;

    let latency = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    let labels = [
        ("method", method.to_string()),
        ("path", path),
        ("status", status),
    ];

    metrics::increment_counter!("http_requests_total", &labels);
    metrics::histogram!("http_requests_duration_seconds", latency, &labels);

    response
}
