//! Distributed rate limiter using Chitchat cluster state.
//!
//! This module provides a distributed rate limiter that uses chitchat
//! for gossip-based state synchronization across multiple nodes.

use std::sync::Arc;
use async_trait::async_trait;
use parking_lot::RwLock;
use tracing::{debug, trace};

use crate::grpc::proto::envoy::extensions::common::ratelimit::v3::RateLimitDescriptor;
use crate::grpc::proto::envoy::service::ratelimit::v3::rate_limit_response::{
    Code, DescriptorStatus, RateLimit,
};
use crate::mesh::{Cluster, CounterKey};

use super::counter::TimeWindow;
use super::descriptor::DescriptorKey;
use super::rules::RateLimitConfig;

/// Default rate limit when no specific limit is configured.
const DEFAULT_LIMIT: u64 = 1000;
/// Default time window when no specific window is configured.
const DEFAULT_WINDOW: TimeWindow = TimeWindow::Second;

/// Configuration for a rate limit.
#[derive(Debug, Clone)]
pub struct LimitConfig {
    /// Maximum requests allowed in the time window
    pub limit: u64,
    /// Time window for the limit
    pub window: TimeWindow,
    /// Name/description of this limit
    pub name: Option<String>,
}

impl Default for LimitConfig {
    fn default() -> Self {
        Self {
            limit: DEFAULT_LIMIT,
            window: DEFAULT_WINDOW,
            name: None,
        }
    }
}

/// A distributed rate limiter backed by Chitchat cluster state.
///
/// This rate limiter uses gossip-based state synchronization,
/// allowing rate limits to be shared across multiple instances.
pub struct DistributedRateLimiter {
    /// The cluster for distributed state.
    cluster: Arc<Cluster>,
    /// Rate limit configuration.
    config: RwLock<RateLimitConfig>,
}

impl DistributedRateLimiter {
    /// Create a new distributed rate limiter.
    pub fn new(cluster: Arc<Cluster>) -> Self {
        Self {
            cluster,
            config: RwLock::new(RateLimitConfig::new()),
        }
    }

    /// Create a new distributed rate limiter with configuration.
    pub fn with_config(cluster: Arc<Cluster>, config: RateLimitConfig) -> Self {
        Self {
            cluster,
            config: RwLock::new(config),
        }
    }

    /// Update the rate limit configuration.
    pub fn set_config(&self, config: RateLimitConfig) {
        let mut cfg = self.config.write();
        *cfg = config;
    }

    /// Get the current configuration.
    pub fn config(&self) -> RateLimitConfig {
        self.config.read().clone()
    }

    /// Check the rate limit for a given domain and descriptor.
    ///
    /// This method increments the counter in the cluster state and returns
    /// the status of the rate limit check.
    pub async fn check_rate_limit(
        &self,
        domain: &str,
        descriptor: &RateLimitDescriptor,
        hits: u32,
    ) -> DescriptorStatus {
        let descriptor_key = DescriptorKey::new(domain, descriptor);

        // Get the limit configuration
        let limit_config = self.get_limit_config(domain, descriptor);
        let window_duration_secs = limit_config.window.duration().as_secs();

        // Calculate the window boundary (floor to window start)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let window_start = (now / window_duration_secs) * window_duration_secs;

        let counter_key = CounterKey::new(domain, &descriptor_key.to_string(), window_start);

        trace!(
            domain = %domain,
            descriptor = %descriptor_key,
            window = window_start,
            hits = hits,
            "Checking distributed rate limit"
        );

        // Increment the counter in cluster state
        let current_count = self.cluster.increment_counter(&counter_key, hits as u64).await;

        let within_limit = current_count <= limit_config.limit;
        let remaining = limit_config.limit.saturating_sub(current_count);

        // Calculate time until window reset
        let window_end = window_start + window_duration_secs;
        let duration_until_reset = window_end.saturating_sub(now);

        let code = if within_limit {
            Code::Ok
        } else {
            debug!(
                domain = %domain,
                descriptor = %descriptor_key,
                count = current_count,
                limit = limit_config.limit,
                "Distributed rate limit exceeded"
            );
            Code::OverLimit
        };

        DescriptorStatus {
            code: code.into(),
            current_limit: Some(RateLimit {
                name: limit_config.name.unwrap_or_default(),
                requests_per_unit: limit_config.limit as u32,
                unit: limit_config.window.to_proto(),
            }),
            limit_remaining: remaining as u32,
            duration_until_reset: Some(prost_types::Duration {
                seconds: duration_until_reset as i64,
                nanos: 0,
            }),
            quota_bucket: None,
        }
    }

    /// Get the limit configuration for a descriptor.
    fn get_limit_config(&self, domain: &str, descriptor: &RateLimitDescriptor) -> LimitConfig {
        // Check if there's an override in the descriptor itself
        if let Some(ref limit_override) = descriptor.limit {
            let window = TimeWindow::from_proto(limit_override.unit).unwrap_or(DEFAULT_WINDOW);
            return LimitConfig {
                limit: limit_override.requests_per_unit as u64,
                window,
                name: None,
            };
        }

        // Look up configured limits
        let config = self.config.read();
        if let Some(rule) = config.find_limit(domain, descriptor) {
            return LimitConfig {
                limit: rule.requests_per_unit,
                window: rule.unit.into(),
                name: rule.name.clone(),
            };
        }

        // Fall back to default limits
        LimitConfig::default()
    }

    /// Get the current counter value for a descriptor.
    pub async fn get_counter_value(&self, domain: &str, descriptor: &RateLimitDescriptor) -> u64 {
        let descriptor_key = DescriptorKey::new(domain, descriptor);
        let limit_config = self.get_limit_config(domain, descriptor);
        let window_duration_secs = limit_config.window.duration().as_secs();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let window_start = (now / window_duration_secs) * window_duration_secs;

        let counter_key = CounterKey::new(domain, &descriptor_key.to_string(), window_start);
        self.cluster.get_count(&counter_key).await
    }

    /// Get the cluster.
    pub fn cluster(&self) -> &Arc<Cluster> {
        &self.cluster
    }

    /// Get the number of live nodes in the cluster.
    pub async fn live_node_count(&self) -> usize {
        self.cluster.live_node_count().await
    }
}

#[async_trait]
impl super::backend::RateLimiterBackend for DistributedRateLimiter {
    async fn check_rate_limit(
        &self,
        domain: &str,
        descriptor: &RateLimitDescriptor,
        hits: u32,
    ) -> DescriptorStatus {
        self.check_rate_limit(domain, descriptor, hits).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::proto::envoy::extensions::common::ratelimit::v3::rate_limit_descriptor::Entry;
    use crate::mesh::ClusterConfig;
    use std::time::Duration;

    fn create_test_descriptor(key: &str, value: &str) -> RateLimitDescriptor {
        RateLimitDescriptor {
            entries: vec![Entry {
                key: key.to_string(),
                value: value.to_string(),
            }],
            limit: None,
        }
    }

    fn test_cluster_config(port: u16) -> ClusterConfig {
        let addr: std::net::SocketAddr = ([127, 0, 0, 1], port).into();
        ClusterConfig {
            node_id: format!("test-node-{}", port),
            listen_addr: addr,
            advertise_addr: addr,
            seed_nodes: Vec::new(),
            cluster_id: "test-cluster".to_string(),
            gossip_interval: Duration::from_millis(50),
            dead_node_grace_period: Duration::from_secs(60),
            cache_ttl: Duration::from_millis(100), // Short TTL for tests
        }
    }

    #[tokio::test]
    async fn test_distributed_limiter_creation() {
        let config = test_cluster_config(18946);
        let cluster = Arc::new(Cluster::start(config).await.unwrap());

        {
            let limiter = DistributedRateLimiter::new(cluster.clone());
            assert_eq!(limiter.live_node_count().await, 1); // Just ourselves
        }

        // Cleanup - limiter dropped, so we can unwrap
        Arc::try_unwrap(cluster).unwrap().shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_distributed_limiter_check() {
        let config = test_cluster_config(18947);
        let cluster = Arc::new(Cluster::start(config).await.unwrap());

        {
            let limiter = DistributedRateLimiter::new(cluster.clone());
            let descriptor = create_test_descriptor("test", "value");

            let status = limiter.check_rate_limit("domain", &descriptor, 1).await;
            assert_eq!(status.code(), Code::Ok);
        }

        Arc::try_unwrap(cluster).unwrap().shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_distributed_limiter_with_config() {
        let cluster_config = test_cluster_config(18948);
        let cluster = Arc::new(Cluster::start(cluster_config).await.unwrap());

        {
            let yaml = r#"
domain: test_domain
descriptors:
  - key: api_key
    rate_limit:
      requests_per_unit: 5
      unit: second
"#;
            let config = RateLimitConfig::from_yaml(yaml).unwrap();
            let limiter = DistributedRateLimiter::with_config(cluster.clone(), config);
            let descriptor = create_test_descriptor("api_key", "my_key");

            // Make 5 requests (should all be OK)
            for i in 1..=5 {
                let status = limiter.check_rate_limit("test_domain", &descriptor, 1).await;
                assert_eq!(status.code(), Code::Ok, "Request {} should be OK", i);
            }

            // 6th request should be over limit
            let status = limiter.check_rate_limit("test_domain", &descriptor, 1).await;
            assert_eq!(status.code(), Code::OverLimit);
        }

        Arc::try_unwrap(cluster).unwrap().shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_distributed_limiter_counter_value() {
        let config = test_cluster_config(18949);
        let cluster = Arc::new(Cluster::start(config).await.unwrap());

        {
            let limiter = DistributedRateLimiter::new(cluster.clone());
            let descriptor = create_test_descriptor("test", "value");

            limiter.check_rate_limit("domain", &descriptor, 5).await;

            let count = limiter.get_counter_value("domain", &descriptor).await;
            assert_eq!(count, 5);
        }

        Arc::try_unwrap(cluster).unwrap().shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_distributed_limiter_cluster_sync() {
        // Start first node
        let config1 = test_cluster_config(18950);
        let cluster1 = Arc::new(Cluster::start(config1).await.unwrap());

        // Start second node with first as seed
        let mut config2 = test_cluster_config(18951);
        config2.seed_nodes = vec!["127.0.0.1:18950".to_string()];
        let cluster2 = Arc::new(Cluster::start(config2).await.unwrap());

        // Give them time to discover each other
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify both nodes see each other
        assert_eq!(cluster1.live_node_count().await, 2);
        assert_eq!(cluster2.live_node_count().await, 2);

        // Use the raw cluster increment for this test to avoid window timing issues
        let key = crate::mesh::CounterKey::new("domain", "test|value", 1000);

        // Increment on cluster1
        let count1 = cluster1.increment_counter(&key, 5).await;
        assert_eq!(count1, 5);

        // Give time for gossip
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Cluster2 should see the count
        let count2 = cluster2.get_count(&key).await;
        assert_eq!(count2, 5, "Cluster 2 should see cluster 1's increment");

        // Increment on cluster2
        cluster2.increment_counter(&key, 3).await;

        // Give time for gossip
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Both should see total of 8
        assert_eq!(cluster1.get_count(&key).await, 8);
        assert_eq!(cluster2.get_count(&key).await, 8);

        // Cleanup
        Arc::try_unwrap(cluster1).unwrap().shutdown().await.unwrap();
        Arc::try_unwrap(cluster2).unwrap().shutdown().await.unwrap();
    }
}
