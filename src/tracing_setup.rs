use opentelemetry::trace::TracerProvider;
use opentelemetry::KeyValue;
use opentelemetry_otlp::{HttpExporterBuilder, WithExportConfig};
use opentelemetry_sdk::{trace::TracerProvider as SdkTracerProvider, Resource};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, Layer};
use std::fs::OpenOptions;

pub fn init_tracing() {
    init_tracing_with_service("tenex-tui");
}

pub fn init_tracing_with_service(service_name: &str) {
    // Check if file logging is enabled via environment variable
    let file_logging = std::env::var("TENEX_LOG_FILE").ok();

    // Use HTTP on port 4318 (the port exposed by Jaeger docker container)
    // Full path required for OTLP HTTP
    let exporter = HttpExporterBuilder::default()
        .with_endpoint("http://localhost:4318/v1/traces")
        .build_span_exporter()
        .expect("Failed to create OTLP exporter");

    let resource = Resource::new([
        KeyValue::new("service.name", service_name.to_string()),
        KeyValue::new("telemetry.sdk.language", "rust"),
    ]);

    // Use batch exporter for async export (requires tokio runtime)
    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .build();

    // Register globally so shutdown_tracing() can flush it
    opentelemetry::global::set_tracer_provider(provider.clone());

    let tracer = provider.tracer(service_name.to_string());
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

    // Build registry with optional file logging
    let registry = tracing_subscriber::registry()
        .with(telemetry.with_filter(tracing_subscriber::filter::LevelFilter::INFO));

    if let Some(log_path) = file_logging {
        // Add file logging layer for debugging freezes
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .expect("Failed to open log file");

        let file_layer = fmt::layer()
            .with_writer(file)
            .with_ansi(false)
            .with_target(true)
            .with_thread_ids(true)
            .with_filter(tracing_subscriber::filter::LevelFilter::DEBUG);

        registry.with(file_layer).init();
        eprintln!("File logging enabled: {}", log_path);
    } else {
        registry.init();
    }
}

/// Shutdown the tracer provider, flushing any pending spans
pub fn shutdown_tracing() {
    opentelemetry::global::shutdown_tracer_provider();
}
