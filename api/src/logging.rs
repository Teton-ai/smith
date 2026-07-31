//! Log/panic initialisation. `LOG_FORMAT=json` emits newline-delimited JSON with no ANSI
//! (one line per event) for structured log aggregators; anything else falls back to
//! human-readable output for local dev.
use std::backtrace::Backtrace;
use tracing_subscriber::{
    EnvFilter, Layer, Registry,
    fmt::{self, time::UtcTime},
    prelude::*,
};

/// Must be called before anything that can panic, including the very first line of `main`:
/// `sentry`'s panic integration (enabled via the `panic` feature) chains to whatever hook was
/// installed before it runs, so our JSON panic log still fires even when Sentry also reports it.
pub fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let json = std::env::var("LOG_FORMAT").is_ok_and(|v| v.eq_ignore_ascii_case("json"));

    tracing_subscriber::registry()
        .with(fmt_layer(json))
        .with(filter)
        .init();

    install_panic_hook();
}

fn fmt_layer(json: bool) -> Box<dyn Layer<Registry> + Send + Sync> {
    if json {
        fmt::layer()
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(false)
            .with_timer(UtcTime::rfc_3339())
            .with_ansi(false)
            .with_target(true)
            .with_file(true)
            .with_line_number(true)
            .boxed()
    } else {
        fmt::layer()
            .pretty()
            .with_timer(UtcTime::rfc_3339())
            .with_target(true)
            .with_file(true)
            .with_line_number(true)
            .boxed()
    }
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        let message = panic_info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| panic_info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic payload".to_string());

        let location = panic_info
            .location()
            .map(|l| l.to_string())
            .unwrap_or_else(|| "unknown location".to_string());

        tracing::error!(
            panic.message = %message,
            panic.location = %location,
            panic.backtrace = %Backtrace::force_capture(),
            "process panicked"
        );
    }));
}
