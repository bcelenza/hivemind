//! Configuration management for Hivemind.

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// Main configuration for the Hivemind service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HivemindConfig {
    /// Server configuration
    #[serde(default)]
    pub server: ServerConfig,

    /// Rate limiting configuration
    #[serde(default)]
    pub rate_limiting: RateLimitingConfig,

    /// Mesh configuration
    #[serde(default)]
    pub mesh: MeshConfig,
}

impl Default for HivemindConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            rate_limiting: RateLimitingConfig::default(),
            mesh: MeshConfig::default(),
        }
    }
}

/// Server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// gRPC server address
    #[serde(default = "default_grpc_addr")]
    pub grpc_addr: SocketAddr,

    /// Metrics server port
    #[serde(default = "default_metrics_port")]
    pub metrics_port: u16,

    /// Admin server port
    #[serde(default = "default_admin_port")]
    pub admin_port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            grpc_addr: default_grpc_addr(),
            metrics_port: default_metrics_port(),
            admin_port: default_admin_port(),
        }
    }
}

fn default_grpc_addr() -> SocketAddr {
    "127.0.0.1:8081".parse().unwrap()
}

fn default_metrics_port() -> u16 {
    9090
}

fn default_admin_port() -> u16 {
    8080
}

/// Rate limiting configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitingConfig {
    /// Path to rate limit rules configuration file
    pub config_path: Option<String>,

    /// Configuration reload interval in seconds
    #[serde(default = "default_reload_interval")]
    pub config_reload_interval_secs: u64,

    /// Local cache size for rate limit counters
    #[serde(default = "default_cache_size")]
    pub local_cache_size: usize,

    /// Staleness threshold in milliseconds
    #[serde(default = "default_staleness_threshold")]
    pub staleness_threshold_ms: u64,
}

impl Default for RateLimitingConfig {
    fn default() -> Self {
        Self {
            config_path: None,
            config_reload_interval_secs: default_reload_interval(),
            local_cache_size: default_cache_size(),
            staleness_threshold_ms: default_staleness_threshold(),
        }
    }
}

fn default_reload_interval() -> u64 {
    60
}

fn default_cache_size() -> usize {
    10000
}

fn default_staleness_threshold() -> u64 {
    500
}

/// Mesh networking configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshConfig {
    /// Enable mesh networking for distributed rate limiting.
    #[serde(default = "default_mesh_enabled")]
    pub enabled: bool,

    /// Node ID (auto-generated if not specified).
    pub node_id: Option<String>,

    /// Address to bind the mesh transport to.
    #[serde(default = "default_mesh_bind_addr")]
    pub bind_addr: SocketAddr,

    /// Bootstrap peer addresses to connect to on startup.
    #[serde(default)]
    pub bootstrap_peers: Vec<String>,

    /// Gossip interval in milliseconds.
    #[serde(default = "default_gossip_interval")]
    pub gossip_interval_ms: u64,

    /// Sync interval in milliseconds.
    #[serde(default = "default_sync_interval")]
    pub sync_interval_ms: u64,

    /// Maximum number of peers.
    #[serde(default = "default_max_peers")]
    pub max_peers: usize,

    /// Health check interval in milliseconds.
    #[serde(default = "default_health_check_interval")]
    pub health_check_interval_ms: u64,

    /// Time after which a peer is considered suspect (milliseconds).
    #[serde(default = "default_suspect_timeout")]
    pub suspect_timeout_ms: u64,

    /// Time after which a suspect peer is considered failed (milliseconds).
    #[serde(default = "default_failed_timeout")]
    pub failed_timeout_ms: u64,
}

impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            enabled: default_mesh_enabled(),
            node_id: None,
            bind_addr: default_mesh_bind_addr(),
            bootstrap_peers: Vec::new(),
            gossip_interval_ms: default_gossip_interval(),
            sync_interval_ms: default_sync_interval(),
            max_peers: default_max_peers(),
            health_check_interval_ms: default_health_check_interval(),
            suspect_timeout_ms: default_suspect_timeout(),
            failed_timeout_ms: default_failed_timeout(),
        }
    }
}

fn default_mesh_enabled() -> bool {
    false
}

fn default_mesh_bind_addr() -> SocketAddr {
    "0.0.0.0:7946".parse().unwrap()
}

fn default_gossip_interval() -> u64 {
    100
}

fn default_sync_interval() -> u64 {
    1000
}

fn default_max_peers() -> usize {
    100
}

fn default_health_check_interval() -> u64 {
    1000
}

fn default_suspect_timeout() -> u64 {
    5000
}

fn default_failed_timeout() -> u64 {
    15000
}

impl HivemindConfig {
    /// Load configuration from a file path.
    pub fn from_file(path: &str) -> crate::error::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: HivemindConfig = serde_yaml::from_str(&contents)
            .map_err(|e| crate::error::HivemindError::Config(e.to_string()))?;
        Ok(config)
    }
}
