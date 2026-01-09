//! Rate limiting logic and state management.

mod limiter;
mod counter;
mod descriptor;

pub use limiter::RateLimiter;
pub use counter::{RateLimitCounter, TimeWindow};
pub use descriptor::DescriptorKey;
