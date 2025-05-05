//! OpenTelemetry and Tracing initialization logic.

use crate::{config::Config, util}; // Use crate path
use opentelemetry::global as otel_global;
use opentelemetry::metrics::{Meter, MeterProvider as _};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{metrics::SdkMeterProvider, runtime, trace as sdktrace, Resource};
use tracing::{error, info};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

// --- OTLP Initialization ---
fn init_otel(cfg: &Config) -> Option<(SdkMeterProvider, sdktrace::Tracer, Meter)> {
    let otel_endpoint = cfg.otel_endpoint.as_ref()?;
    info!(endpoint = %otel_endpoint, "Initializing OpenTelemetry exporter...");

    // --- Trace Pipeline ---
    let trace_exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(otel_endpoint);
    let trace_config = sdktrace::config().with_resource(Resource::new(vec![
        opentelemetry::KeyValue::new("service.name", "intercept-bounce"),
        opentelemetry::KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
    ]));
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(trace_exporter)
        .with_trace_config(trace_config)
        .install_batch(runtime::TokioCurrentThread)
        .map_err(|e| error!(error = %e, "Failed to initialize OTLP trace pipeline"))
        .ok()?;

    // --- Metrics Pipeline ---
    let metrics_exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(otel_endpoint);
    let meter_provider = opentelemetry_otlp::new_pipeline()
        .metrics(runtime::TokioCurrentThread)
        .with_exporter(metrics_exporter)
        .build()
        .map_err(|e| error!(error = %e, "Failed to initialize OTLP metrics pipeline"))
        .ok()?;

    otel_global::set_meter_provider(meter_provider.clone());
    let meter = otel_global::meter_provider().meter("intercept-bounce");
    info!("OpenTelemetry exporter initialized successfully.");
    Some((meter_provider, tracer, meter))
}

/// Initialize tracing subscriber (fmt layer + optional OTLP layer).
/// Returns the OTLP Meter if OTLP is configured and initialized successfully.
pub fn init_tracing(cfg: &Config) -> Option<Meter> {
    let fmt_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(cfg.verbose)
        .with_level(true);

    let filter = EnvFilter::try_new(&cfg.log_filter).unwrap_or_else(|e| {
        eprintln!("Warning: Invalid RUST_LOG '{}': {e}", cfg.log_filter);
        EnvFilter::new("intercept_bounce=info") // Default filter on parse error
    });

    // Base subscriber registry
    let registry_base = tracing_subscriber::registry().with(fmt_layer).with(filter);

    // Conditionally add OTLP layer and initialize the subscriber
    let otel_meter = if let Some((_meter_provider, tracer, meter)) = init_otel(cfg) {
        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
        registry_base.with(otel_layer).init();
        Some(meter) // OTLP initialized, return the meter
    } else {
        registry_base.init(); // Initialize without OTLP
        None
    };

    info!(
        version = env!("CARGO_PKG_VERSION"),
        // Use option_env! for git sha to avoid build errors outside git repo
        git_sha = option_env!("VERGEN_GIT_SHA_SHORT").unwrap_or("unknown"),
        build_ts = env!("VERGEN_BUILD_TIMESTAMP"),
        "intercept-bounce starting"
    );

    info!(debounce = %util::format_duration(cfg.debounce_time()),
        near_miss = %util::format_duration(cfg.near_miss_threshold()),
        log_interval = %util::format_duration(cfg.log_interval()),
        log_all = cfg.log_all_events,
        log_bounces = cfg.log_bounces,
        stats_json = cfg.stats_json,
        verbose = cfg.verbose,
        log_filter = %cfg.log_filter,
        otel_endpoint = %cfg.otel_endpoint.as_deref().unwrap_or("<None>"),
        "Configuration loaded");

    otel_meter
}
