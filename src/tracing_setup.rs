use opentelemetry::trace::TracerProvider;
use opentelemetry::KeyValue;
use opentelemetry_otlp::{HttpExporterBuilder, WithExportConfig};
use opentelemetry_sdk::{runtime, trace::TracerProvider as SdkTracerProvider, Resource};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};

pub fn init_tracing() {
    // Use HTTP on port 4318 (the port exposed by Jaeger docker container)
    let exporter = HttpExporterBuilder::default()
        .with_endpoint("http://localhost:4318")
        .build_span_exporter()
        .expect("Failed to create OTLP exporter");

    let resource = Resource::new([
        KeyValue::new("service.name", "tenex-tui"),
        KeyValue::new("telemetry.sdk.language", "rust"),
    ]);

    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter, runtime::Tokio)
        .build();

    let tracer = provider.tracer("tenex-tui");
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

    tracing_subscriber::registry()
        .with(telemetry.with_filter(tracing_subscriber::filter::LevelFilter::INFO))
        .init();
}
