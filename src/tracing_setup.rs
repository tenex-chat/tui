use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::TracerProvider as SdkTracerProvider;
use opentelemetry_stdout::SpanExporter;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};

pub fn init_tracing() {
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(SpanExporter::default())
        .build();

    let tracer = provider.tracer("tenex-tui");
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

    tracing_subscriber::registry()
        .with(telemetry.with_filter(tracing_subscriber::filter::LevelFilter::INFO))
        .init();
}
