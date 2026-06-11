//! Telemetry and observability for the Aegis engine.
//!
//! Provides structured logging via `tracing` and optional OpenTelemetry export
//! behind the `telemetry` feature flag.

use tracing_subscriber::EnvFilter;

#[cfg(feature = "telemetry")]
use tracing_subscriber::prelude::*;

/// Guard that flushes telemetry on drop.
pub struct TelemetryGuard {
    /// Whether OpenTelemetry was initialized.
    pub otel_enabled: bool,
    #[cfg(feature = "telemetry")]
    provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        #[cfg(feature = "telemetry")]
        if let Some(provider) = self.provider.take() {
            let _ = provider.shutdown();
        }
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

    TelemetryGuard {
        otel_enabled: false,
        #[cfg(feature = "telemetry")]
        provider: None,
    }
}

/// Initialize OpenTelemetry with OTLP export.
///
/// Requires the `telemetry` feature. Sets up a batch span processor
/// exporting to the endpoint specified by `OTEL_EXPORTER_OTLP_ENDPOINT`
/// (defaults to `http://localhost:4317`).
#[cfg(feature = "telemetry")]
pub fn init_otel() -> Result<TelemetryGuard, Box<dyn std::error::Error>> {
    use opentelemetry::global;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use opentelemetry_sdk::Resource;

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .build()?;

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(Resource::builder().with_service_name("aegis").build())
        .build();

    global::set_tracer_provider(provider.clone());

    let otel_layer = tracing_opentelemetry::layer();

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true);

    tracing_subscriber::registry()
        .with(fmt_layer.with_filter(filter))
        .with(otel_layer)
        .init();

    Ok(TelemetryGuard {
        otel_enabled: true,
        provider: Some(provider),
    })
}

/// Span names used throughout the engine.
pub mod spans {
    pub const CHECK: &str = "aegis.check";
    pub const EXPLAIN: &str = "aegis.explain";
    pub const WRITE: &str = "aegis.write";
    pub const DELETE: &str = "aegis.delete";
    pub const QUERY: &str = "aegis.query";
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
