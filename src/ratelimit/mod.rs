//! Rate limiting logic and state management.

mod limiter;
mod counter;
mod descriptor;
mod rules;
mod distributed;
mod backend;

pub use limiter::{RateLimiter, LimitConfig};
pub use counter::{RateLimitCounter, TimeWindow};
pub use descriptor::DescriptorKey;
pub use rules::{RateLimitConfig, DomainConfig, DescriptorConfig, RateLimitRule, TimeUnit};
pub use distributed::DistributedRateLimiter;
pub use backend::RateLimiterBackend;
