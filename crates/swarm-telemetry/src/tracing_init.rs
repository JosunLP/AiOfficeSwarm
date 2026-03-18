//! Tracing subscriber initialization.

use swarm_config::model::{LogFormat, TelemetryConfig};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialize the global `tracing` subscriber.
///
/// This function should be called once at application startup. Calling it
/// multiple times is harmless (subsequent calls are no-ops due to the
/// `try_init` semantics of `tracing-subscriber`).
///
/// The subscriber is configured from [`TelemetryConfig`]:
/// - `log_level`: sets the `EnvFilter` directive.
/// - `log_format`: selects between human-readable text and JSON output.
pub fn init_tracing(config: &TelemetryConfig) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.log_level));

    match config.log_format {
        LogFormat::Json => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().json())
                .try_init()
                .ok();
        }
        LogFormat::Text => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer())
                .try_init()
                .ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_config::model::TelemetryConfig;

    #[test]
    fn init_tracing_does_not_panic() {
        // Should be a no-op if already initialized.
        let cfg = TelemetryConfig::default();
        init_tracing(&cfg);
        init_tracing(&cfg); // second call should be a no-op
    }
}
