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
}

impl Default for HivemindConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            rate_limiting: RateLimitingConfig::default(),
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

impl HivemindConfig {
    /// Load configuration from a file path.
    pub fn from_file(path: &str) -> crate::error::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: HivemindConfig = serde_yaml::from_str(&contents)
            .map_err(|e| crate::error::HivemindError::Config(e.to_string()))?;
        Ok(config)
    }
}
