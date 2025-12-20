use opentelemetry::trace::TracerProvider;
use opentelemetry::KeyValue;
use opentelemetry_otlp::{HttpExporterBuilder, WithExportConfig};
use opentelemetry_sdk::{trace::TracerProvider as SdkTracerProvider, Resource};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};

pub fn init_tracing() {
    init_tracing_with_service("tenex-tui");
}

pub fn init_tracing_with_service(service_name: &str) {
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

    tracing_subscriber::registry()
        .with(telemetry.with_filter(tracing_subscriber::filter::LevelFilter::INFO))
        .init();
}

/// Shutdown the tracer provider, flushing any pending spans
pub fn shutdown_tracing() {
    opentelemetry::global::shutdown_tracer_provider();
}
