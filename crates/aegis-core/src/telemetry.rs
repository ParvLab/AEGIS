//! Telemetry and observability for the Aegis engine.
//!
//! Provides structured logging via `tracing` and optional OpenTelemetry export
//! behind the `telemetry` feature flag.

use tracing_subscriber::EnvFilter;

/// Guard that flushes telemetry on drop.
pub struct TelemetryGuard {
    _private: (),
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        #[cfg(feature = "telemetry")]
        flush_otel();
    }
}

/// Initialize the structured logger with sensible defaults.
///
/// Uses `RUST_LOG` environment variable to control verbosity (default: `info`).
/// Call this once at application startup.
pub fn init_logger() -> TelemetryGuard {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(true)
        .with_thread_ids(true)
        .init();

    TelemetryGuard { _private: () }
}

/// Initialize OpenTelemetry with OTLP export.
///
/// Requires the `telemetry` feature. Sets up a batch span processor
/// exporting to the endpoint specified by `OTEL_EXPORTER_OTLP_ENDPOINT`
/// (defaults to `http://localhost:4317`).
#[cfg(feature = "telemetry")]
pub fn init_otel() -> Result<TelemetryGuard, Box<dyn std::error::Error>> {
    use opentelemetry::global;
    use opentelemetry_otlp::WithExportConfig;

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .build()?;

    let provider = opentelemetry::sdk::trace::TracerProvider::builder()
        .with_batch_exporter(exporter, opentelemetry::runtime::Tokio)
        .build();

    global::set_tracer_provider(provider);

    // Re-register the tracing subscriber with OTLP layer
    let otel_layer = tracing_opentelemetry::layer();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(true)
        .with_thread_ids(true)
        .finish()
        .with(otel_layer)
        .init();

    Ok(TelemetryGuard { _private: () })
}

#[cfg(feature = "telemetry")]
fn flush_otel() {
    opentelemetry::global::shutdown_tracer_provider();
}

/// Span names used throughout the engine.
pub mod spans {
    pub const CHECK: &str = "aegis.check";
    pub const EXPLAIN: &str = "aegis.explain";
    pub const WRITE: &str = "aegis.write";
    pub const DELETE: &str = "aegis.delete";
    pub const WATCH_SEND: &str = "aegis.watch_send";
    pub const HOOK_TRIGGER: &str = "aegis.hook_trigger";
    pub const CACHE_LOOKUP: &str = "aegis.cache_lookup";
    pub const TRAVERSAL: &str = "aegis.traversal";
}

/// Key names for span attributes.
pub mod keys {
    pub const SUBJECT: &str = "aegis.subject";
    pub const RELATION: &str = "aegis.relation";
    pub const RESOURCE: &str = "aegis.resource";
    pub const PERMISSION: &str = "aegis.permission";
    pub const ALLOWED: &str = "aegis.allowed";
    pub const REVISION: &str = "aegis.revision";
    pub const DURATION_MS: &str = "aegis.duration_ms";
    pub const CACHE_HIT: &str = "aegis.cache_hit";
    pub const ERROR: &str = "aegis.error";
    pub const BACKEND: &str = "aegis.backend";
}
