use tokio::sync::OnceCell;
use tracing::metadata::LevelFilter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, EnvFilter};

const DEFAULT_LEVEL: LevelFilter = LevelFilter::INFO;
// Define a OnceCell to ensure the configuration is initialized only once
static TRACING_INITIALIZED: OnceCell<()> = OnceCell::const_new();

pub async fn configure_tracing() {
    TRACING_INITIALIZED
        .get_or_init(|| async {
            let fmt_layer = fmt::layer().compact().with_target(true);
            let level_filter_layer =
                EnvFilter::builder().with_default_directive(DEFAULT_LEVEL.into()).from_env_lossy();

            // This sets a single subscriber to all of the threads. We may want to implement
            // different subscriber for some threads and use set_global_default instead
            // of init.
            tracing_subscriber::registry().with(fmt_layer).with(level_filter_layer).init();
            tracing::info!("Tracing has been successfully initialized.");
        })
        .await;
}
