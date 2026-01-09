//! Core rate limiter implementation.

use std::collections::HashMap;
use std::sync::RwLock;
use async_trait::async_trait;
use tracing::{debug, trace};

use crate::grpc::proto::envoy::extensions::common::ratelimit::v3::RateLimitDescriptor;
use crate::grpc::proto::envoy::service::ratelimit::v3::rate_limit_response::{
    Code, DescriptorStatus, RateLimit,
};

use super::backend::RateLimiterBackend;
use super::counter::{RateLimitCounter, TimeWindow};
use super::descriptor::DescriptorKey;
use super::rules::RateLimitConfig;

/// Default rate limit when no specific limit is configured.
const DEFAULT_LIMIT: u64 = 1000;
/// Default time window when no specific window is configured.
const DEFAULT_WINDOW: TimeWindow = TimeWindow::Second;

/// The core rate limiter that manages rate limit counters.
///
/// This struct is thread-safe and can be shared across multiple tasks.
pub struct RateLimiter {
    /// Rate limit counters indexed by descriptor key
    counters: RwLock<HashMap<DescriptorKey, RateLimitCounter>>,
    /// Configured rate limits loaded from configuration
    config: RwLock<RateLimitConfig>,
}

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

impl RateLimiter {
    /// Create a new rate limiter with default settings.
    pub fn new() -> Self {
        Self {
            counters: RwLock::new(HashMap::new()),
            config: RwLock::new(RateLimitConfig::new()),
        }
    }

    /// Create a new rate limiter with the given configuration.
    pub fn with_config(config: RateLimitConfig) -> Self {
        Self {
            counters: RwLock::new(HashMap::new()),
            config: RwLock::new(config),
        }
    }

    /// Update the rate limit configuration.
    ///
    /// This does not clear existing counters - they will continue with
    /// their current state but may use new limits on their next check.
    pub fn set_config(&self, config: RateLimitConfig) {
        let mut cfg = self.config.write().unwrap();
        *cfg = config;
    }

    /// Get the current configuration.
    pub fn config(&self) -> RateLimitConfig {
        self.config.read().unwrap().clone()
    }

    /// Check the rate limit for a given domain and descriptor.
    ///
    /// This method increments the counter and returns the status of the rate limit check.
    pub async fn check_rate_limit(
        &self,
        domain: &str,
        descriptor: &RateLimitDescriptor,
        hits: u32,
    ) -> DescriptorStatus {
        let key = DescriptorKey::new(domain, descriptor);

        trace!(
            key = %key,
            hits = hits,
            "Checking rate limit"
        );

        // Get or create the counter for this descriptor
        let (within_limit, current_limit, remaining, duration_until_reset) = {
            let mut counters = self.counters.write().unwrap();

            let counter = counters
                .entry(key.clone())
                .or_insert_with(|| {
                    // Look up the configured limit for this descriptor
                    let config = self.get_limit_config(domain, descriptor);
                    debug!(
                        key = %key,
                        limit = config.limit,
                        window = ?config.window,
                        "Creating new rate limit counter"
                    );
                    RateLimitCounter::new(config.limit, config.window)
                });

            let within_limit = counter.increment(hits);
            let current_limit = counter.limit();
            let remaining = counter.remaining();
            let duration_until_reset = counter.duration_until_reset();
            let window = counter.window();

            (
                within_limit,
                RateLimit {
                    name: String::new(),
                    requests_per_unit: current_limit as u32,
                    unit: window.to_proto(),
                },
                remaining,
                duration_until_reset,
            )
        };

        let code = if within_limit {
            Code::Ok
        } else {
            debug!(
                key = %key,
                "Rate limit exceeded"
            );
            Code::OverLimit
        };

        DescriptorStatus {
            code: code.into(),
            current_limit: Some(current_limit),
            limit_remaining: remaining as u32,
            duration_until_reset: Some(prost_types::Duration {
                seconds: duration_until_reset.as_secs() as i64,
                nanos: duration_until_reset.subsec_nanos() as i32,
            }),
            quota_bucket: None,
        }
    }

    /// Get the limit configuration for a descriptor.
    ///
    /// This looks up the limit in the following order:
    /// 1. Override specified in the descriptor itself
    /// 2. Configured limit from the rate limit configuration
    /// 3. Default limit
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

        // Look up configured limits based on domain and descriptor entries
        let config = self.config.read().unwrap();
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

    /// Get the current counter value for a descriptor key.
    ///
    /// Returns `None` if no counter exists for the key.
    pub fn get_counter_value(&self, domain: &str, descriptor: &RateLimitDescriptor) -> Option<u64> {
        let key = DescriptorKey::new(domain, descriptor);
        let counters = self.counters.read().unwrap();
        counters.get(&key).map(|c| c.current_count())
    }

    /// Clear all counters.
    ///
    /// This is primarily useful for testing.
    pub fn clear(&self) {
        let mut counters = self.counters.write().unwrap();
        counters.clear();
    }

    /// Get the number of active counters.
    pub fn counter_count(&self) -> usize {
        let counters = self.counters.read().unwrap();
        counters.len()
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RateLimiterBackend for RateLimiter {
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

    fn create_test_descriptor(key: &str, value: &str) -> RateLimitDescriptor {
        RateLimitDescriptor {
            entries: vec![Entry {
                key: key.to_string(),
                value: value.to_string(),
            }],
            limit: None,
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_creation() {
        let limiter = RateLimiter::new();
        assert_eq!(limiter.counter_count(), 0);
    }

    #[tokio::test]
    async fn test_check_rate_limit_creates_counter() {
        let limiter = RateLimiter::new();
        let descriptor = create_test_descriptor("test", "value");

        let status = limiter.check_rate_limit("domain", &descriptor, 1).await;

        assert_eq!(status.code(), Code::Ok);
        assert_eq!(limiter.counter_count(), 1);
    }

    #[tokio::test]
    async fn test_check_rate_limit_increments() {
        let limiter = RateLimiter::new();
        let descriptor = create_test_descriptor("test", "value");

        // First request
        limiter.check_rate_limit("domain", &descriptor, 1).await;
        let count = limiter.get_counter_value("domain", &descriptor);
        assert_eq!(count, Some(1));

        // Second request
        limiter.check_rate_limit("domain", &descriptor, 1).await;
        let count = limiter.get_counter_value("domain", &descriptor);
        assert_eq!(count, Some(2));
    }

    #[tokio::test]
    async fn test_check_rate_limit_with_override() {
        let limiter = RateLimiter::new();

        use crate::grpc::proto::envoy::extensions::common::ratelimit::v3::rate_limit_descriptor::RateLimitOverride;

        let descriptor = RateLimitDescriptor {
            entries: vec![Entry {
                key: "test".to_string(),
                value: "value".to_string(),
            }],
            limit: Some(RateLimitOverride {
                requests_per_unit: 5,
                unit: TimeWindow::Second.to_proto(),
            }),
        };

        // Make 5 requests (should all be OK)
        for _ in 0..5 {
            let status = limiter.check_rate_limit("domain", &descriptor, 1).await;
            assert_eq!(status.code(), Code::Ok);
        }

        // 6th request should be over limit
        let status = limiter.check_rate_limit("domain", &descriptor, 1).await;
        assert_eq!(status.code(), Code::OverLimit);
    }

    #[tokio::test]
    async fn test_different_domains_have_separate_counters() {
        let limiter = RateLimiter::new();
        let descriptor = create_test_descriptor("key", "value");

        limiter.check_rate_limit("domain1", &descriptor, 5).await;
        limiter.check_rate_limit("domain2", &descriptor, 3).await;

        assert_eq!(limiter.get_counter_value("domain1", &descriptor), Some(5));
        assert_eq!(limiter.get_counter_value("domain2", &descriptor), Some(3));
    }

    #[tokio::test]
    async fn test_clear_counters() {
        let limiter = RateLimiter::new();
        let descriptor = create_test_descriptor("test", "value");

        limiter.check_rate_limit("domain", &descriptor, 1).await;
        assert_eq!(limiter.counter_count(), 1);

        limiter.clear();
        assert_eq!(limiter.counter_count(), 0);
    }

    #[tokio::test]
    async fn test_rate_limiter_with_config() {
        let yaml = r#"
domain: test_domain
descriptors:
  - key: api_key
    rate_limit:
      requests_per_unit: 5
      unit: second
"#;
        let config = RateLimitConfig::from_yaml(yaml).unwrap();
        let limiter = RateLimiter::with_config(config);
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

    #[tokio::test]
    async fn test_rate_limiter_hierarchical_config() {
        let yaml = r#"
domain: test_domain
descriptors:
  - key: source_cluster
    rate_limit:
      requests_per_unit: 100
      unit: second
    descriptors:
      - key: destination_cluster
        value: critical
        rate_limit:
          requests_per_unit: 10
          unit: second
"#;
        let config = RateLimitConfig::from_yaml(yaml).unwrap();
        let limiter = RateLimiter::with_config(config);

        // Test general source_cluster limit (100/s)
        let general_descriptor = create_test_descriptor("source_cluster", "frontend");
        for _ in 0..100 {
            let status = limiter.check_rate_limit("test_domain", &general_descriptor, 1).await;
            assert_eq!(status.code(), Code::Ok);
        }
        let status = limiter.check_rate_limit("test_domain", &general_descriptor, 1).await;
        assert_eq!(status.code(), Code::OverLimit);

        // Test specific critical destination (10/s)
        let critical_descriptor = RateLimitDescriptor {
            entries: vec![
                Entry {
                    key: "source_cluster".to_string(),
                    value: "frontend".to_string(),
                },
                Entry {
                    key: "destination_cluster".to_string(),
                    value: "critical".to_string(),
                },
            ],
            limit: None,
        };

        for _ in 0..10 {
            let status = limiter.check_rate_limit("test_domain", &critical_descriptor, 1).await;
            assert_eq!(status.code(), Code::Ok);
        }
        let status = limiter.check_rate_limit("test_domain", &critical_descriptor, 1).await;
        assert_eq!(status.code(), Code::OverLimit);
    }

    #[tokio::test]
    async fn test_update_config() {
        let limiter = RateLimiter::new();
        let descriptor = create_test_descriptor("api_key", "test");

        // With default config (1000/s limit), make some requests
        for _ in 0..100 {
            let status = limiter.check_rate_limit("test_domain", &descriptor, 1).await;
            assert_eq!(status.code(), Code::Ok);
        }

        // Update config with stricter limit
        let yaml = r#"
domain: test_domain
descriptors:
  - key: api_key
    rate_limit:
      requests_per_unit: 5
      unit: second
"#;
        let config = RateLimitConfig::from_yaml(yaml).unwrap();
        limiter.set_config(config);

        // The existing counter continues - it already has 100 hits
        // New descriptors will use the new config
        let new_descriptor = create_test_descriptor("api_key", "new_key");
        for _ in 0..5 {
            let status = limiter.check_rate_limit("test_domain", &new_descriptor, 1).await;
            assert_eq!(status.code(), Code::Ok);
        }
        let status = limiter.check_rate_limit("test_domain", &new_descriptor, 1).await;
        assert_eq!(status.code(), Code::OverLimit);
    }

    #[tokio::test]
    async fn test_unconfigured_domain_uses_defaults() {
        let yaml = r#"
domain: configured_domain
descriptors:
  - key: api_key
    rate_limit:
      requests_per_unit: 5
      unit: second
"#;
        let config = RateLimitConfig::from_yaml(yaml).unwrap();
        let limiter = RateLimiter::with_config(config);

        // Request to unconfigured domain should use default limit (1000/s)
        let descriptor = create_test_descriptor("api_key", "test");
        for _ in 0..100 {
            let status = limiter.check_rate_limit("unconfigured_domain", &descriptor, 1).await;
            assert_eq!(status.code(), Code::Ok);
        }
    }
}
