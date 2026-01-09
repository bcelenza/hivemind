//! Descriptor key generation and handling.

use crate::grpc::proto::envoy::extensions::common::ratelimit::v3::RateLimitDescriptor;

/// A key that uniquely identifies a rate limit descriptor.
///
/// The key is composed of the domain and all descriptor entries,
/// serialized in a consistent order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DescriptorKey {
    /// The domain this descriptor belongs to
    pub domain: String,
    /// Serialized key-value pairs from the descriptor
    pub entries: Vec<(String, String)>,
}

impl DescriptorKey {
    /// Create a new descriptor key from a domain and descriptor.
    pub fn new(domain: &str, descriptor: &RateLimitDescriptor) -> Self {
        let entries: Vec<(String, String)> = descriptor
            .entries
            .iter()
            .map(|e| (e.key.clone(), e.value.clone()))
            .collect();

        Self {
            domain: domain.to_string(),
            entries,
        }
    }

    /// Convert the descriptor key to a string representation.
    ///
    /// This is useful for logging and debugging.
    pub fn to_string_key(&self) -> String {
        let entries_str: Vec<String> = self
            .entries
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        
        format!("{}:{}", self.domain, entries_str.join(","))
    }
}

impl std::fmt::Display for DescriptorKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string_key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::proto::envoy::extensions::common::ratelimit::v3::rate_limit_descriptor::Entry;

    #[test]
    fn test_descriptor_key_creation() {
        let descriptor = RateLimitDescriptor {
            entries: vec![
                Entry {
                    key: "source".to_string(),
                    value: "client_a".to_string(),
                },
                Entry {
                    key: "destination".to_string(),
                    value: "service_b".to_string(),
                },
            ],
            limit: None,
        };

        let key = DescriptorKey::new("test_domain", &descriptor);
        
        assert_eq!(key.domain, "test_domain");
        assert_eq!(key.entries.len(), 2);
        assert_eq!(key.entries[0], ("source".to_string(), "client_a".to_string()));
        assert_eq!(key.entries[1], ("destination".to_string(), "service_b".to_string()));
    }

    #[test]
    fn test_descriptor_key_to_string() {
        let descriptor = RateLimitDescriptor {
            entries: vec![
                Entry {
                    key: "key1".to_string(),
                    value: "value1".to_string(),
                },
            ],
            limit: None,
        };

        let key = DescriptorKey::new("domain", &descriptor);
        assert_eq!(key.to_string_key(), "domain:key1=value1");
    }

    #[test]
    fn test_descriptor_key_equality() {
        let descriptor = RateLimitDescriptor {
            entries: vec![Entry {
                key: "test".to_string(),
                value: "value".to_string(),
            }],
            limit: None,
        };

        let key1 = DescriptorKey::new("domain", &descriptor);
        let key2 = DescriptorKey::new("domain", &descriptor);
        
        assert_eq!(key1, key2);
    }
}
