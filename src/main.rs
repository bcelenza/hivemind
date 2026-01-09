use std::sync::Arc;
use tokio::signal;
use tracing::{info, Level};
use tracing_subscriber;

use hivemind::config::HivemindConfig;
use hivemind::grpc::GrpcServer;
use hivemind::ratelimit::RateLimiter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_target(false)
        .with_thread_ids(true)
        .init();

    info!("Starting Hivemind Rate Limiting Service");
    info!("Version: {}", env!("CARGO_PKG_VERSION"));

    // Load configuration (use defaults for now)
    let config = HivemindConfig::default();
    info!(grpc_addr = %config.server.grpc_addr, "Configuration loaded");

    // Initialize the rate limiter
    let rate_limiter = Arc::new(RateLimiter::new());
    info!("Rate limiter initialized");

    // Create and start the gRPC server
    let grpc_server = GrpcServer::new(config.server.grpc_addr, rate_limiter);

    info!("Starting gRPC server on {}", config.server.grpc_addr);

    // Run the server with graceful shutdown on Ctrl+C
    grpc_server
        .serve_with_shutdown(shutdown_signal())
        .await?;

    info!("Hivemind Rate Limiting Service stopped");
    Ok(())
}

/// Wait for a shutdown signal (Ctrl+C or SIGTERM).
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C, initiating graceful shutdown");
        }
        _ = terminate => {
            info!("Received SIGTERM, initiating graceful shutdown");
        }
    }
}
