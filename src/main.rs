use std::sync::Arc;
use clap::Parser;
use tokio::signal;
use tracing::{info, warn, Level};
use tracing_subscriber;

use hivemind::config::HivemindConfig;
use hivemind::grpc::GrpcServer;
use hivemind::mesh::{Cluster, ClusterConfig};
use hivemind::ratelimit::{RateLimiter, RateLimitConfig, DistributedRateLimiter};

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

    /// Enable mesh networking for distributed rate limiting
    #[arg(long = "mesh", default_value = "false")]
    mesh_enabled: bool,

    /// Mesh node ID (auto-generated if not specified)
    #[arg(long = "node-id")]
    node_id: Option<String>,

    /// Mesh bind address
    #[arg(long = "mesh-addr", default_value = "0.0.0.0:7946")]
    mesh_addr: String,

    /// Bootstrap peer addresses (comma-separated)
    #[arg(long = "peers")]
    bootstrap_peers: Option<String>,
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

    // Initialize and run the gRPC server with the appropriate rate limiter
    if args.mesh_enabled {
        // Parse mesh configuration from CLI
        let mesh_addr: std::net::SocketAddr = args.mesh_addr.parse()
            .expect("Invalid mesh address");

        let seed_nodes: Vec<String> = args.bootstrap_peers
            .map(|s| s.split(',')
                .map(|addr| addr.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect())
            .unwrap_or_default();

        let cluster_config = ClusterConfig {
            node_id: args.node_id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            listen_addr: mesh_addr,
            advertise_addr: mesh_addr,
            seed_nodes,
            cluster_id: "hivemind".to_string(),
            ..Default::default()
        };

        let cluster = Arc::new(Cluster::start(cluster_config).await
            .expect("Failed to start cluster"));

        let distributed_limiter = Arc::new(DistributedRateLimiter::with_config(
            cluster.clone(),
            rate_limit_config,
        ));

        info!(
            node_id = %cluster.node_id(),
            mesh_addr = %args.mesh_addr,
            "Distributed rate limiter initialized with cluster"
        );

        let grpc_server = GrpcServer::with_distributed_limiter(config.server.grpc_addr, distributed_limiter);

        info!("Starting gRPC server on {}", config.server.grpc_addr);
        grpc_server.serve_with_shutdown(shutdown_signal()).await?;
    } else {
        let rate_limiter = Arc::new(RateLimiter::with_config(rate_limit_config));
        info!("Local rate limiter initialized");

        let grpc_server = GrpcServer::new(config.server.grpc_addr, rate_limiter);

        info!("Starting gRPC server on {}", config.server.grpc_addr);
        grpc_server.serve_with_shutdown(shutdown_signal()).await?;
    }

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
