use std::sync::Arc;
use clap::Parser;
use tokio::signal;
use tracing::{info, warn, Level};
use tracing_subscriber;

use hivemind::config::HivemindConfig;
use hivemind::grpc::GrpcServer;
use hivemind::ratelimit::{RateLimiter, RateLimitConfig};

/// Hivemind - Distributed rate limiting service for Envoy Proxy
#[derive(Parser, Debug)]
#[command(name = "hivemind")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the rate limit configuration file
    #[arg(short = 'c', long = "config")]
    config: Option<String>,

    /// gRPC server address
    #[arg(short = 'a', long = "addr", default_value = "127.0.0.1:8081")]
    addr: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse command-line arguments
    let args = Args::parse();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_target(false)
        .with_thread_ids(true)
        .init();

    info!("Starting Hivemind Rate Limiting Service");
    info!("Version: {}", env!("CARGO_PKG_VERSION"));

    // Load configuration with CLI overrides
    let mut config = HivemindConfig::default();

    // Override with CLI arguments
    if let Some(ref config_path) = args.config {
        config.rate_limiting.config_path = Some(config_path.clone());
    }
    if let Ok(addr) = args.addr.parse() {
        config.server.grpc_addr = addr;
    }

    info!(grpc_addr = %config.server.grpc_addr, "Configuration loaded");

    // Load rate limit rules from configuration file/directory
    let rate_limit_config = load_rate_limit_config(&config);

    // Initialize the rate limiter with configuration
    let rate_limiter = Arc::new(RateLimiter::with_config(rate_limit_config));
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

/// Load rate limit configuration from the configured file path.
fn load_rate_limit_config(config: &HivemindConfig) -> RateLimitConfig {
    if let Some(ref config_path) = config.rate_limiting.config_path {
        match RateLimitConfig::from_file(config_path) {
            Ok(cfg) => {
                info!(
                    path = %config_path,
                    domain_count = cfg.domains.len(),
                    "Rate limit configuration loaded"
                );
                return cfg;
            }
            Err(e) => {
                warn!(
                    path = %config_path,
                    error = %e,
                    "Failed to load rate limit configuration, using defaults"
                );
            }
        }
    } else {
        info!("No rate limit configuration path specified, using defaults");
    }

    RateLimitConfig::new()
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
