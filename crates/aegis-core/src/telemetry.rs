//! Telemetry and observability for the Aegis engine.
//!
//! Provides structured logging via `tracing` and optional OpenTelemetry export
//! behind the `telemetry` feature flag.

use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "telemetry")]
use tracing_subscriber::EnvFilter;

#[cfg(feature = "telemetry")]
use tracing_subscriber::prelude::*;

/// Global metrics counters (used regardless of telemetry feature).
pub(crate) static METRIC_CHECK_TOTAL: AtomicU64 = AtomicU64::new(0);
pub(crate) static METRIC_CHECK_ALLOWED: AtomicU64 = AtomicU64::new(0);
pub(crate) static METRIC_CHECK_DENIED: AtomicU64 = AtomicU64::new(0);
pub(crate) static METRIC_CHECK_ERROR: AtomicU64 = AtomicU64::new(0);
pub(crate) static METRIC_CACHE_SIZE: AtomicU64 = AtomicU64::new(0);
pub(crate) static METRIC_CACHE_HITS: AtomicU64 = AtomicU64::new(0);
pub(crate) static METRIC_CACHE_MISSES: AtomicU64 = AtomicU64::new(0);

// ── Gauge statics ──────────────────────────────────────────────────────────
pub(crate) static METRIC_SCHEMA_VERSION: AtomicU64 = AtomicU64::new(0);
pub(crate) static METRIC_REVISION_CURRENT: AtomicU64 = AtomicU64::new(0);

#[cfg(feature = "telemetry")]
pub(crate) static METRIC_GRAPH_TUPLE_COUNT: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "telemetry")]
pub(crate) static METRIC_GRAPH_TENANT_COUNT: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "telemetry")]
pub(crate) static METRIC_STORAGE_CONNECTIONS_ACTIVE: AtomicU64 = AtomicU64::new(0);

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
    #[cfg(feature = "telemetry")]
    {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .with_target(true)
            .with_thread_ids(true)
            .init();
    }

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
    pub const QUERY: &str = "aegis.query";
    pub const CACHE_LOOKUP: &str = "aegis.cache_lookup";
}

// ── Metrics counters ──────────────────────────────────────────────────────

/// Increment total check counter.
pub fn inc_check_total() {
    METRIC_CHECK_TOTAL.fetch_add(1, Ordering::Relaxed);
}

/// Increment allowed check counter.
pub fn inc_check_allowed() {
    METRIC_CHECK_ALLOWED.fetch_add(1, Ordering::Relaxed);
}

/// Increment denied check counter.
pub fn inc_check_denied() {
    METRIC_CHECK_DENIED.fetch_add(1, Ordering::Relaxed);
}

/// Increment error check counter.
pub fn inc_check_error() {
    METRIC_CHECK_ERROR.fetch_add(1, Ordering::Relaxed);
}

/// Set cache size gauge.
pub fn set_cache_size(val: u64) {
    METRIC_CACHE_SIZE.store(val, Ordering::Relaxed);
}

/// Record a cache hit.
pub fn inc_cache_hit() {
    METRIC_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
}

/// Record a cache miss.
pub fn inc_cache_miss() {
    METRIC_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
}

/// Update schema version gauge.
pub fn update_schema_version(val: u64) {
    METRIC_SCHEMA_VERSION.store(val, Ordering::Relaxed);
}

/// Update current revision gauge.
pub fn update_revision_current(val: u64) {
    METRIC_REVISION_CURRENT.store(val, Ordering::Relaxed);
}

#[cfg(feature = "telemetry")]
pub mod otel_metrics {
    //! OpenTelemetry metric instruments, available only with the `telemetry` feature.
    use std::sync::atomic::Ordering;
    use std::sync::OnceLock;

    use opentelemetry::global;
    use opentelemetry::metrics::Meter;
    use opentelemetry::metrics::MeterProvider as _;
    use opentelemetry::KeyValue;

    use super::{
        METRIC_GRAPH_TENANT_COUNT, METRIC_GRAPH_TUPLE_COUNT, METRIC_REVISION_CURRENT,
        METRIC_SCHEMA_VERSION, METRIC_STORAGE_CONNECTIONS_ACTIVE,
    };

    static CUSTOM_PROVIDER: OnceLock<Option<opentelemetry_sdk::metrics::SdkMeterProvider>> =
        OnceLock::new();

    pub fn init_provider(provider: opentelemetry_sdk::metrics::SdkMeterProvider) {
        let _ = CUSTOM_PROVIDER.set(Some(provider));
    }

    fn meter() -> Meter {
        if let Some(Some(provider)) = CUSTOM_PROVIDER.get() {
            return provider.meter("aegis");
        }
        global::meter("aegis")
    }

    fn ensure_gauges() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let m = meter();
            let _ = m
                .u64_observable_gauge("aegis.graph.tuple_count")
                .with_description("Number of tuples in storage")
                .with_callback(|observer| {
                    observer.observe(
                        METRIC_GRAPH_TUPLE_COUNT.load(Ordering::Relaxed),
                        &[],
                    );
                })
                .build();
            let _ = m
                .u64_observable_gauge("aegis.graph.tenant_count")
                .with_description("Number of tenants/namespaces")
                .with_callback(|observer| {
                    observer.observe(
                        METRIC_GRAPH_TENANT_COUNT.load(Ordering::Relaxed),
                        &[],
                    );
                })
                .build();
            let _ = m
                .u64_observable_gauge("aegis.storage.connections.active")
                .with_description("Active storage connections")
                .with_callback(|observer| {
                    observer.observe(
                        METRIC_STORAGE_CONNECTIONS_ACTIVE.load(Ordering::Relaxed),
                        &[],
                    );
                })
                .build();
            let _ = m
                .u64_observable_gauge("aegis.schema.version")
                .with_description("Current schema version")
                .with_callback(|observer| {
                    observer.observe(
                        METRIC_SCHEMA_VERSION.load(Ordering::Relaxed),
                        &[],
                    );
                })
                .build();
            let _ = m
                .u64_observable_gauge("aegis.revision.current")
                .with_description("Current revision number")
                .with_callback(|observer| {
                    observer.observe(
                        METRIC_REVISION_CURRENT.load(Ordering::Relaxed),
                        &[],
                    );
                })
                .build();
        });
    }

    /// Counter: total number of check calls.
    pub fn record_check_total(allowed: bool) {
        ensure_gauges();
        let counter = meter()
            .u64_counter("aegis.check.total")
            .with_description("Total number of authorization checks")
            .build();
        counter.add(1, &[KeyValue::new("allowed", if allowed { "true" } else { "false" })]);
    }

    /// Counter: checks that resulted in allow.
    pub fn record_check_allowed() {
        ensure_gauges();
        let counter = meter()
            .u64_counter("aegis.check.allowed")
            .with_description("Number of allowed authorization checks")
            .build();
        counter.add(1, &[]);
    }

    /// Counter: checks that resulted in deny.
    pub fn record_check_denied() {
        ensure_gauges();
        let counter = meter()
            .u64_counter("aegis.check.denied")
            .with_description("Number of denied authorization checks")
            .build();
        counter.add(1, &[]);
    }

    /// Counter: checks that resulted in error.
    pub fn record_check_error() {
        ensure_gauges();
        let counter = meter()
            .u64_counter("aegis.check.error")
            .with_description("Number of authorization checks that errored")
            .build();
        counter.add(1, &[]);
    }

    /// Histogram: check duration in milliseconds.
    pub fn record_check_duration_ms(duration_ms: f64, allowed: bool) {
        ensure_gauges();
        let histogram = meter()
            .f64_histogram("aegis.check.duration_ms")
            .with_description("Duration of authorization checks in milliseconds")
            .with_unit("ms")
            .build();
        histogram.record(duration_ms, &[KeyValue::new("allowed", if allowed { "true" } else { "false" })]);
    }

    /// Gauge (up-down counter): current cache size.
    pub fn record_cache_size(size: i64) {
        ensure_gauges();
        let gauge = meter()
            .i64_up_down_counter("aegis.cache.size")
            .with_description("Current number of entries in the decision cache")
            .build();
        gauge.add(size, &[]);
    }

    /// Histogram: cache hit ratio (0.0–1.0).
    pub fn record_cache_hit_ratio(ratio: f64) {
        ensure_gauges();
        let histogram = meter()
            .f64_histogram("aegis.cache.hit_ratio")
            .with_description("Cache hit ratio observed over check intervals")
            .with_unit("ratio")
            .build();
        histogram.record(ratio, &[]);
    }
}

#[cfg(feature = "telemetry")]
#[cfg(test)]
mod tests {
    use super::otel_metrics;
    use crate::engine::GraphEngine;
    use crate::storage::sqlite::{SqliteConfig, SqliteStorage};
    use crate::storage::StorageBackend;
    use crate::types::{
        Relation, RelationshipTuple, ResourceId, Schema, SubjectId,
    };
    use opentelemetry_sdk::metrics::InMemoryMetricExporter;
    use opentelemetry_sdk::metrics::PeriodicReader;
    use opentelemetry_sdk::metrics::SdkMeterProvider;

    fn make_engine_with_provider(
        provider: SdkMeterProvider,
    ) -> GraphEngine {
        let schema = Schema {
            schema_version: 1,
            namespace: "test".to_string(),
            types: {
                let mut types = std::collections::HashMap::new();
                let mut relations = std::collections::HashMap::new();
                relations.insert(
                    "owner".to_string(),
                    crate::types::schema::RelationDef {
                        inherit_from: vec![],
                        description: None,
                    },
                );
                relations.insert(
                    "viewer".to_string(),
                    crate::types::schema::RelationDef {
                        inherit_from: vec![],
                        description: None,
                    },
                );
                let mut permissions = std::collections::HashMap::new();
                permissions.insert(
                    "read".to_string(),
                    crate::types::schema::PermissionDef {
                        union_of: vec!["viewer".to_string(), "owner".to_string()],
                        condition: None,
                        description: None,
                    },
                );
                permissions.insert(
                    "admin".to_string(),
                    crate::types::schema::PermissionDef {
                        union_of: vec!["owner".to_string()],
                        condition: None,
                        description: None,
                    },
                );
                types.insert(
                    "repo".to_string(),
                    crate::types::schema::TypeDef {
                        relations,
                        permissions,
                    },
                );
                types
            },
        };

        let mut storage = SqliteStorage::new(SqliteConfig::in_memory()).unwrap();
        storage.initialize().unwrap();
        GraphEngine::new(Box::new(storage), schema).with_meter_provider(provider)
    }

    #[test]
    fn test_in_memory_metrics_exporter() {
        let exporter = InMemoryMetricExporter::default();
        let reader = PeriodicReader::builder(exporter.clone()).build();
        let provider = SdkMeterProvider::builder()
            .with_reader(reader)
            .build();

        let engine = make_engine_with_provider(provider);

        // Write a tuple and run a check
        let subject = SubjectId::new("user:alice").unwrap();
        let resource = ResourceId::new("repo:fluxbus").unwrap();

        engine
            .write(&RelationshipTuple::new(
                subject.clone(),
                Relation::new("owner").unwrap(),
                resource.clone(),
            ))
            .unwrap();

        let _ = engine
            .check(&subject, "read", &resource, None)
            .unwrap();

        // Verify statics were updated
        assert!(
            crate::telemetry::METRIC_CHECK_TOTAL.load(std::sync::atomic::Ordering::Relaxed) >= 1,
            "check.total should be at least 1"
        );
    }

    /// Verifies the otel_metrics module compiles and basic instrument creation works.
    #[test]
    fn test_metrics_compile() {
        let provider = SdkMeterProvider::builder().build();
        otel_metrics::init_provider(provider);
        otel_metrics::record_check_total(true);
        otel_metrics::record_check_allowed();
    }
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
