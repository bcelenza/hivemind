//! Rate limit rules configuration and matching.
//!
//! This module handles loading and matching rate limit rules from configuration.
//! It supports Envoy's rate limit configuration format with hierarchical descriptor matching.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tracing::info;

use super::counter::TimeWindow;
use crate::error::{HivemindError, Result};
use crate::grpc::proto::envoy::extensions::common::ratelimit::v3::RateLimitDescriptor;

/// A complete rate limit configuration containing multiple domains.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Map of domain name to domain configuration
    #[serde(default)]
    pub domains: HashMap<String, DomainConfig>,
}

/// Configuration for a single rate limit domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainConfig {
    /// The domain name
    pub domain: String,
    /// Top-level descriptors for this domain
    #[serde(default)]
    pub descriptors: Vec<DescriptorConfig>,
}

/// Configuration for a rate limit descriptor.
///
/// Descriptors form a tree structure where each node can have:
/// - A key to match against
/// - An optional value to match (if not present, matches any value)
/// - An optional rate limit to apply at this level
/// - Child descriptors for more specific matching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescriptorConfig {
    /// The key to match
    pub key: String,
    /// Optional value to match (if not set, matches any value for this key)
    #[serde(default)]
    pub value: Option<String>,
    /// Rate limit to apply at this level
    #[serde(default)]
    pub rate_limit: Option<RateLimitRule>,
    /// Child descriptors for more specific matching
    #[serde(default)]
    pub descriptors: Vec<DescriptorConfig>,
}

/// A rate limit rule specifying the limit and time window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitRule {
    /// Number of requests allowed per unit of time
    pub requests_per_unit: u64,
    /// The time unit
    pub unit: TimeUnit,
    /// Optional name/description for this limit
    #[serde(default)]
    pub name: Option<String>,
}

/// Time unit for rate limits (matches Envoy's configuration format).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeUnit {
    Second,
    Minute,
    Hour,
    Day,
}

impl From<TimeUnit> for TimeWindow {
    fn from(unit: TimeUnit) -> Self {
        match unit {
            TimeUnit::Second => TimeWindow::Second,
            TimeUnit::Minute => TimeWindow::Minute,
            TimeUnit::Hour => TimeWindow::Hour,
            TimeUnit::Day => TimeWindow::Day,
        }
    }
}

impl RateLimitConfig {
    /// Create an empty configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load configuration from a YAML file.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        info!(path = %path.display(), "Loading rate limit configuration");

        let contents = std::fs::read_to_string(path)?;
        Self::from_yaml(&contents)
    }

    /// Load configuration from a YAML string.
    pub fn from_yaml(yaml: &str) -> Result<Self> {
        // First, try to parse as a single domain config (Envoy's typical format)
        if let Ok(domain_config) = serde_yaml::from_str::<DomainConfig>(yaml) {
            let mut config = RateLimitConfig::new();
            config.domains.insert(domain_config.domain.clone(), domain_config);
            return Ok(config);
        }

        // Otherwise, try to parse as a full config with multiple domains
        serde_yaml::from_str(yaml)
            .map_err(|e| HivemindError::Config(format!("Failed to parse rate limit config: {}", e)))
    }

    /// Get the configuration for a specific domain.
    pub fn get_domain(&self, domain: &str) -> Option<&DomainConfig> {
        self.domains.get(domain)
    }

    /// Find the matching rate limit rule for a descriptor within a domain.
    ///
    /// This performs hierarchical matching, where more specific matches take precedence.
    pub fn find_limit(
        &self,
        domain: &str,
        descriptor: &RateLimitDescriptor,
    ) -> Option<&RateLimitRule> {
        let domain_config = self.get_domain(domain)?;
        domain_config.find_limit(descriptor)
    }
}

impl DomainConfig {
    /// Find the matching rate limit rule for a descriptor.
    pub fn find_limit(&self, descriptor: &RateLimitDescriptor) -> Option<&RateLimitRule> {
        Self::find_limit_in_descriptors(&self.descriptors, &descriptor.entries, 0)
    }

    /// Recursively find a matching rate limit in the descriptor tree.
    fn find_limit_in_descriptors<'a>(
        configs: &'a [DescriptorConfig],
        entries: &[crate::grpc::proto::envoy::extensions::common::ratelimit::v3::rate_limit_descriptor::Entry],
        entry_index: usize,
    ) -> Option<&'a RateLimitRule> {
        if entry_index >= entries.len() {
            return None;
        }

        let entry = &entries[entry_index];
        let mut best_match: Option<&RateLimitRule> = None;

        for config in configs {
            // Check if this config matches the current entry
            if config.key != entry.key {
                continue;
            }

            // Check value match (if value is specified in config)
            let value_matches = match &config.value {
                Some(v) => v == &entry.value,
                None => true, // No value specified means match any value
            };

            if !value_matches {
                continue;
            }

            // This config matches - check for more specific matches in children
            if entry_index + 1 < entries.len() && !config.descriptors.is_empty() {
                if let Some(child_limit) = Self::find_limit_in_descriptors(
                    &config.descriptors,
                    entries,
                    entry_index + 1,
                ) {
                    // Found a more specific match
                    return Some(child_limit);
                }
            }

            // Use this level's rate limit if it exists and we haven't found a more specific one
            if let Some(ref limit) = config.rate_limit {
                best_match = Some(limit);
            }
        }

        best_match
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::proto::envoy::extensions::common::ratelimit::v3::rate_limit_descriptor::Entry;

    fn create_descriptor(entries: &[(&str, &str)]) -> RateLimitDescriptor {
        RateLimitDescriptor {
            entries: entries
                .iter()
                .map(|(k, v)| Entry {
                    key: k.to_string(),
                    value: v.to_string(),
                })
                .collect(),
            limit: None,
        }
    }

    #[test]
    fn test_parse_simple_config() {
        let yaml = r#"
domain: test_domain
descriptors:
  - key: source_cluster
    rate_limit:
      requests_per_unit: 100
      unit: second
"#;
        let config = RateLimitConfig::from_yaml(yaml).unwrap();
        assert!(config.domains.contains_key("test_domain"));
        assert_eq!(config.domains["test_domain"].descriptors.len(), 1);
    }

    #[test]
    fn test_parse_hierarchical_config() {
        let yaml = r#"
domain: test_domain
descriptors:
  - key: source_cluster
    value: cluster_a
    rate_limit:
      requests_per_unit: 100
      unit: second
    descriptors:
      - key: destination_cluster
        value: cluster_b
        rate_limit:
          requests_per_unit: 50
          unit: second
"#;
        let config = RateLimitConfig::from_yaml(yaml).unwrap();
        let domain = &config.domains["test_domain"];
        assert_eq!(domain.descriptors.len(), 1);
        assert_eq!(domain.descriptors[0].descriptors.len(), 1);
    }

    #[test]
    fn test_find_limit_simple() {
        let yaml = r#"
domain: test_domain
descriptors:
  - key: api_key
    rate_limit:
      requests_per_unit: 1000
      unit: minute
"#;
        let config = RateLimitConfig::from_yaml(yaml).unwrap();
        let descriptor = create_descriptor(&[("api_key", "some_key")]);

        let limit = config.find_limit("test_domain", &descriptor);
        assert!(limit.is_some());
        let limit = limit.unwrap();
        assert_eq!(limit.requests_per_unit, 1000);
        assert_eq!(limit.unit, TimeUnit::Minute);
    }

    #[test]
    fn test_find_limit_with_value_match() {
        let yaml = r#"
domain: test_domain
descriptors:
  - key: source_cluster
    value: premium
    rate_limit:
      requests_per_unit: 10000
      unit: second
  - key: source_cluster
    value: basic
    rate_limit:
      requests_per_unit: 100
      unit: second
"#;
        let config = RateLimitConfig::from_yaml(yaml).unwrap();

        // Premium tier
        let descriptor = create_descriptor(&[("source_cluster", "premium")]);
        let limit = config.find_limit("test_domain", &descriptor).unwrap();
        assert_eq!(limit.requests_per_unit, 10000);

        // Basic tier
        let descriptor = create_descriptor(&[("source_cluster", "basic")]);
        let limit = config.find_limit("test_domain", &descriptor).unwrap();
        assert_eq!(limit.requests_per_unit, 100);
    }

    #[test]
    fn test_find_limit_hierarchical() {
        let yaml = r#"
domain: test_domain
descriptors:
  - key: source_cluster
    rate_limit:
      requests_per_unit: 1000
      unit: second
    descriptors:
      - key: destination_cluster
        value: critical_service
        rate_limit:
          requests_per_unit: 100
          unit: second
"#;
        let config = RateLimitConfig::from_yaml(yaml).unwrap();

        // Just source_cluster - should get top-level limit
        let descriptor = create_descriptor(&[("source_cluster", "any")]);
        let limit = config.find_limit("test_domain", &descriptor).unwrap();
        assert_eq!(limit.requests_per_unit, 1000);

        // source_cluster + destination_cluster - should get more specific limit
        let descriptor = create_descriptor(&[
            ("source_cluster", "any"),
            ("destination_cluster", "critical_service"),
        ]);
        let limit = config.find_limit("test_domain", &descriptor).unwrap();
        assert_eq!(limit.requests_per_unit, 100);
    }

    #[test]
    fn test_find_limit_no_match() {
        let yaml = r#"
domain: test_domain
descriptors:
  - key: api_key
    rate_limit:
      requests_per_unit: 1000
      unit: minute
"#;
        let config = RateLimitConfig::from_yaml(yaml).unwrap();

        // Different key - no match
        let descriptor = create_descriptor(&[("other_key", "value")]);
        let limit = config.find_limit("test_domain", &descriptor);
        assert!(limit.is_none());

        // Different domain - no match
        let descriptor = create_descriptor(&[("api_key", "value")]);
        let limit = config.find_limit("other_domain", &descriptor);
        assert!(limit.is_none());
    }

    #[test]
    fn test_find_limit_any_value() {
        let yaml = r#"
domain: test_domain
descriptors:
  - key: remote_address
    rate_limit:
      requests_per_unit: 50
      unit: second
"#;
        let config = RateLimitConfig::from_yaml(yaml).unwrap();

        // Should match any value for remote_address
        let descriptor = create_descriptor(&[("remote_address", "192.168.1.1")]);
        let limit = config.find_limit("test_domain", &descriptor).unwrap();
        assert_eq!(limit.requests_per_unit, 50);

        let descriptor = create_descriptor(&[("remote_address", "10.0.0.1")]);
        let limit = config.find_limit("test_domain", &descriptor).unwrap();
        assert_eq!(limit.requests_per_unit, 50);
    }

    #[test]
    fn test_time_unit_conversion() {
        assert_eq!(TimeWindow::from(TimeUnit::Second), TimeWindow::Second);
        assert_eq!(TimeWindow::from(TimeUnit::Minute), TimeWindow::Minute);
        assert_eq!(TimeWindow::from(TimeUnit::Hour), TimeWindow::Hour);
        assert_eq!(TimeWindow::from(TimeUnit::Day), TimeWindow::Day);
    }
}
