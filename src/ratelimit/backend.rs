//! Rate limiter trait for abstracting local and distributed implementations.

use async_trait::async_trait;

use crate::grpc::proto::envoy::extensions::common::ratelimit::v3::RateLimitDescriptor;
use crate::grpc::proto::envoy::service::ratelimit::v3::rate_limit_response::DescriptorStatus;

/// Trait for rate limiter implementations.
///
/// This trait abstracts over both the local `RateLimiter` and the
/// `DistributedRateLimiter` to allow the gRPC service to work with either.
#[async_trait]
pub trait RateLimiterBackend: Send + Sync {
    /// Check the rate limit for a given domain and descriptor.
    async fn check_rate_limit(
        &self,
        domain: &str,
        descriptor: &RateLimitDescriptor,
        hits: u32,
    ) -> DescriptorStatus;
}
