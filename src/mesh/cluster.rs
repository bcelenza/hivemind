//! Cluster management using Chitchat.
//!
//! This module wraps the chitchat library to provide cluster membership,
//! failure detection, and state gossip for distributed rate limiting.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use chitchat::transport::UdpTransport;
use chitchat::{
    spawn_chitchat, ChitchatConfig, ChitchatHandle, ChitchatId, FailureDetectorConfig,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{debug, info};

/// Errors that can occur in cluster operations.
#[derive(Debug, Error)]
pub enum ClusterError {
    #[error("Failed to start cluster: {0}")]
    StartError(String),
    #[error("Failed to join cluster: {0}")]
    JoinError(String),
}

/// Configuration for the cluster.
#[derive(Debug, Clone)]
pub struct ClusterConfig {
    /// Unique node identifier.
    pub node_id: String,
    /// The address to listen on for gossip.
    pub listen_addr: SocketAddr,
    /// The address to advertise to other nodes.
    pub advertise_addr: SocketAddr,
    /// Seed nodes to bootstrap cluster membership.
    pub seed_nodes: Vec<String>,
    /// Cluster identifier (nodes must have matching cluster IDs).
    pub cluster_id: String,
    /// How often to gossip with peers.
    pub gossip_interval: Duration,
    /// Grace period before considering a dead node's state deletable.
    pub dead_node_grace_period: Duration,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        let default_addr: SocketAddr = "0.0.0.0:7946".parse().unwrap();
        Self {
            node_id: uuid::Uuid::new_v4().to_string(),
            listen_addr: default_addr,
            advertise_addr: default_addr,
            seed_nodes: Vec::new(),
            cluster_id: "hivemind".to_string(),
            gossip_interval: Duration::from_millis(100),
            dead_node_grace_period: Duration::from_secs(3600), // 1 hour
        }
    }
}

/// Key identifying a rate limit counter in the cluster state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CounterKey {
    /// The rate limit domain.
    pub domain: String,
    /// The descriptor key (serialized).
    pub descriptor: String,
    /// The time window (epoch seconds, floored to window boundary).
    pub window: u64,
}

impl CounterKey {
    /// Create a new counter key.
    pub fn new(domain: &str, descriptor: &str, window: u64) -> Self {
        Self {
            domain: domain.to_string(),
            descriptor: descriptor.to_string(),
            window,
        }
    }

    /// Convert to a chitchat key string.
    /// Format: "counter|{domain}|{descriptor}|{window}"
    /// We use | as delimiter since it's less common in domain/descriptor values
    pub fn to_chitchat_key(&self) -> String {
        format!("counter|{}|{}|{}", self.domain, self.descriptor, self.window)
    }

    /// Parse from a chitchat key string.
    pub fn from_chitchat_key(key: &str) -> Option<Self> {
        // Split from the right to handle descriptors containing the delimiter
        if !key.starts_with("counter|") {
            return None;
        }

        let rest = &key[8..]; // Skip "counter|"

        // Find the last | for window
        let last_sep = rest.rfind('|')?;
        let window: u64 = rest[last_sep + 1..].parse().ok()?;

        let before_window = &rest[..last_sep];

        // Find the first | for domain
        let first_sep = before_window.find('|')?;
        let domain = &before_window[..first_sep];
        let descriptor = &before_window[first_sep + 1..];

        Some(Self {
            domain: domain.to_string(),
            descriptor: descriptor.to_string(),
            window,
        })
    }
}

/// The cluster handle for distributed state management.
pub struct Cluster {
    /// Our node ID.
    node_id: String,
    /// Chitchat handle.
    handle: ChitchatHandle,
    /// Configuration (kept for potential future use).
    #[allow(dead_code)]
    config: ClusterConfig,
}

impl std::fmt::Debug for Cluster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cluster")
            .field("node_id", &self.node_id)
            .field("config", &self.config)
            .finish()
    }
}

impl Cluster {
    /// Create and start a new cluster node.
    pub async fn start(config: ClusterConfig) -> Result<Self, ClusterError> {
        info!(
            node_id = %config.node_id,
            listen_addr = %config.listen_addr,
            advertise_addr = %config.advertise_addr,
            seed_nodes = ?config.seed_nodes,
            cluster_id = %config.cluster_id,
            "Starting cluster node"
        );

        let chitchat_id = ChitchatId {
            node_id: config.node_id.clone(),
            generation_id: 0,
            gossip_advertise_addr: config.advertise_addr,
        };

        let chitchat_config = ChitchatConfig {
            chitchat_id,
            cluster_id: config.cluster_id.clone(),
            gossip_interval: config.gossip_interval,
            listen_addr: config.listen_addr,
            seed_nodes: config.seed_nodes.clone(),
            failure_detector_config: FailureDetectorConfig {
                initial_interval: config.gossip_interval,
                ..Default::default()
            },
            marked_for_deletion_grace_period: config.dead_node_grace_period,
            catchup_callback: None,
            extra_liveness_predicate: None,
        };

        let transport = UdpTransport;
        let handle = spawn_chitchat(chitchat_config, Vec::new(), &transport)
            .await
            .map_err(|e| ClusterError::StartError(e.to_string()))?;

        info!("Cluster node started successfully");

        Ok(Self {
            node_id: config.node_id.clone(),
            handle,
            config,
        })
    }

    /// Get our node ID.
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    /// Get the chitchat handle for direct access.
    pub fn chitchat(&self) -> Arc<Mutex<chitchat::Chitchat>> {
        self.handle.chitchat()
    }

    /// Increment a counter and return the total across all nodes.
    ///
    /// This sets our local contribution for the counter key and reads
    /// all other nodes' contributions to compute the total.
    pub async fn increment_counter(&self, key: &CounterKey, amount: u64) -> u64 {
        let chitchat_key = key.to_chitchat_key();
        let chitchat_arc = self.handle.chitchat();
        let mut chitchat = chitchat_arc.lock().await;

        // Get our current local value
        let current_local: u64 = chitchat
            .self_node_state()
            .get(&chitchat_key)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        // Increment and store
        let new_local = current_local + amount;
        chitchat.self_node_state().set(&chitchat_key, new_local.to_string());

        debug!(
            key = %chitchat_key,
            local_value = new_local,
            "Incremented local counter"
        );

        // Sum across all nodes (including ourselves)
        self.sum_counter_internal(&chitchat, &chitchat_key)
    }

    /// Get the total count for a key across all nodes.
    pub async fn get_count(&self, key: &CounterKey) -> u64 {
        let chitchat_key = key.to_chitchat_key();
        let chitchat_arc = self.handle.chitchat();
        let chitchat = chitchat_arc.lock().await;
        self.sum_counter_internal(&chitchat, &chitchat_key)
    }

    /// Internal helper to sum a counter across all nodes.
    fn sum_counter_internal(&self, chitchat: &chitchat::Chitchat, key: &str) -> u64 {
        let mut total: u64 = 0;

        // Sum from all live nodes
        for node_id in chitchat.live_nodes() {
            if let Some(node_state) = chitchat.node_state(node_id) {
                if let Some(value) = node_state.get(key) {
                    if let Ok(count) = value.parse::<u64>() {
                        total += count;
                    }
                }
            }
        }

        total
    }

    /// Get the number of live nodes in the cluster.
    pub async fn live_node_count(&self) -> usize {
        let chitchat_arc = self.handle.chitchat();
        let chitchat = chitchat_arc.lock().await;
        chitchat.live_nodes().count()
    }

    /// Get the IDs of all live nodes.
    pub async fn live_nodes(&self) -> Vec<String> {
        let chitchat_arc = self.handle.chitchat();
        let chitchat = chitchat_arc.lock().await;
        chitchat
            .live_nodes()
            .map(|id| id.node_id.clone())
            .collect()
    }

    /// Shutdown the cluster node gracefully.
    pub async fn shutdown(self) -> Result<(), ClusterError> {
        info!(node_id = %self.node_id, "Shutting down cluster node");
        self.handle
            .shutdown()
            .await
            .map_err(|e| ClusterError::StartError(format!("Shutdown error: {:?}", e)))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn test_config(port: u16) -> ClusterConfig {
        let addr: SocketAddr = ([127, 0, 0, 1], port).into();
        ClusterConfig {
            node_id: format!("test-node-{}", port),
            listen_addr: addr,
            advertise_addr: addr,
            seed_nodes: Vec::new(),
            cluster_id: "test-cluster".to_string(),
            gossip_interval: Duration::from_millis(50),
            dead_node_grace_period: Duration::from_secs(60),
        }
    }

    #[test]
    fn test_counter_key() {
        let key = CounterKey::new("my_domain", "user:123", 1704067200);
        let chitchat_key = key.to_chitchat_key();
        assert_eq!(chitchat_key, "counter|my_domain|user:123|1704067200");

        let parsed = CounterKey::from_chitchat_key(&chitchat_key).unwrap();
        assert_eq!(parsed.domain, "my_domain");
        assert_eq!(parsed.descriptor, "user:123");
        assert_eq!(parsed.window, 1704067200);
    }

    #[test]
    fn test_counter_key_parsing_invalid() {
        assert!(CounterKey::from_chitchat_key("invalid").is_none());
        assert!(CounterKey::from_chitchat_key("counter|only|two").is_none());
        assert!(CounterKey::from_chitchat_key("notcounter|a|b|123").is_none());
    }

    #[tokio::test]
    async fn test_cluster_start() {
        let config = test_config(17946);
        let cluster = Cluster::start(config).await.unwrap();

        assert_eq!(cluster.node_id(), "test-node-17946");
        assert_eq!(cluster.live_node_count().await, 1); // Just ourselves

        cluster.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_cluster_increment_counter() {
        let config = test_config(17947);
        let cluster = Cluster::start(config).await.unwrap();

        let key = CounterKey::new("test", "key1", 1000);

        // Increment should return the new total
        let total = cluster.increment_counter(&key, 5).await;
        assert_eq!(total, 5);

        let total = cluster.increment_counter(&key, 3).await;
        assert_eq!(total, 8);

        // get_count should match
        let count = cluster.get_count(&key).await;
        assert_eq!(count, 8);

        cluster.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_cluster_two_nodes() {
        // Start first node
        let config1 = test_config(17948);
        let cluster1 = Cluster::start(config1).await.unwrap();

        // Start second node with first as seed
        let mut config2 = test_config(17949);
        config2.seed_nodes = vec!["127.0.0.1:17948".to_string()];
        let cluster2 = Cluster::start(config2).await.unwrap();

        // Give them time to discover each other
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Both should see 2 live nodes
        assert_eq!(cluster1.live_node_count().await, 2);
        assert_eq!(cluster2.live_node_count().await, 2);

        // Increment on node 1
        let key = CounterKey::new("test", "shared", 1000);
        cluster1.increment_counter(&key, 10).await;

        // Give time for gossip
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Node 2 should see the count
        let count = cluster2.get_count(&key).await;
        assert_eq!(count, 10);

        // Increment on node 2
        cluster2.increment_counter(&key, 5).await;

        // Give time for gossip
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Both should see total of 15
        assert_eq!(cluster1.get_count(&key).await, 15);
        assert_eq!(cluster2.get_count(&key).await, 15);

        cluster1.shutdown().await.unwrap();
        cluster2.shutdown().await.unwrap();
    }
}
