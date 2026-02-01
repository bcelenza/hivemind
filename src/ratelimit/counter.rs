//! Rate limit counter implementation.
//!
//! This module provides a lock-free rate limit counter using epoch-based windows.
//! The counter packs the window epoch and count into a single atomic u64, enabling
//! fully lock-free operations using compare-and-swap (CAS).
//!
//! ## Design
//!
//! Instead of storing a mutable `window_start` timestamp that requires locking,
//! we compute the current window epoch from a fixed reference point. The state
//! is packed as: `[32-bit epoch][32-bit count]`.
//!
//! ## Limitations
//!
//! - **Maximum count per window**: 4,294,967,295 (u32::MAX). This is sufficient
//!   for most rate limiting use cases. If you need higher limits, consider using
//!   longer time windows or distributing across multiple keys.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Time window for rate limiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeWindow {
    /// Per-second rate limiting
    Second,
    /// Per-minute rate limiting
    Minute,
    /// Per-hour rate limiting
    Hour,
    /// Per-day rate limiting
    Day,
}

impl TimeWindow {
    /// Get the duration of this time window.
    pub fn duration(&self) -> Duration {
        match self {
            TimeWindow::Second => Duration::from_secs(1),
            TimeWindow::Minute => Duration::from_secs(60),
            TimeWindow::Hour => Duration::from_secs(3600),
            TimeWindow::Day => Duration::from_secs(86400),
        }
    }

    /// Convert from the proto enum value.
    pub fn from_proto(unit: i32) -> Option<Self> {
        match unit {
            1 => Some(TimeWindow::Second),
            2 => Some(TimeWindow::Minute),
            3 => Some(TimeWindow::Hour),
            4 => Some(TimeWindow::Day),
            _ => None,
        }
    }

    /// Convert to the proto enum value.
    pub fn to_proto(&self) -> i32 {
        match self {
            TimeWindow::Second => 1,
            TimeWindow::Minute => 2,
            TimeWindow::Hour => 3,
            TimeWindow::Day => 4,
        }
    }
}

/// A rate limit counter that tracks requests within a time window.
///
/// This counter is fully lock-free, using atomic compare-and-swap operations
/// to update state. The window epoch and count are packed into a single `u64`:
/// - Upper 32 bits: window epoch (computed from elapsed time)
/// - Lower 32 bits: request count within the current window
///
/// # Performance
///
/// - **Reads**: Always lock-free, single atomic load
/// - **Writes**: Lock-free CAS loop, only retries on concurrent modification
/// - **Cache-friendly**: Single cache line for all mutable state
///
/// # Limitations
///
/// The maximum count per window is `u32::MAX` (4,294,967,295). For rate limits
/// exceeding this, consider using longer time windows.
pub struct RateLimitCounter {
    /// Packed state: upper 32 bits = window epoch, lower 32 bits = count
    state: AtomicU64,
    /// The limit for this counter (capped at u32::MAX for comparison)
    limit: u64,
    /// Time window for this counter
    window: TimeWindow,
    /// Fixed reference point for computing window epochs
    epoch_start: Instant,
}

impl RateLimitCounter {
    /// Create a new rate limit counter.
    ///
    /// # Arguments
    ///
    /// * `limit` - Maximum requests allowed per window. Values above `u32::MAX`
    ///   will be capped to `u32::MAX` for internal comparisons.
    /// * `window` - The time window for rate limiting.
    pub fn new(limit: u64, window: TimeWindow) -> Self {
        Self {
            state: AtomicU64::new(0),
            limit,
            window,
            epoch_start: Instant::now(),
        }
    }

    /// Compute the current window epoch based on elapsed time from the fixed start.
    #[inline]
    fn current_epoch(&self) -> u32 {
        let elapsed = self.epoch_start.elapsed();
        let window_nanos = self.window.duration().as_nanos() as u64;
        // Safe: window_nanos is always > 0 for valid TimeWindow values
        ((elapsed.as_nanos() as u64) / window_nanos) as u32
    }

    /// Pack epoch and count into a single u64.
    #[inline]
    const fn pack(epoch: u32, count: u32) -> u64 {
        ((epoch as u64) << 32) | (count as u64)
    }

    /// Unpack epoch and count from a u64.
    #[inline]
    const fn unpack(value: u64) -> (u32, u32) {
        let epoch = (value >> 32) as u32;
        let count = value as u32;
        (epoch, count)
    }

    /// Increment the counter and check if the limit has been exceeded.
    ///
    /// This operation is lock-free and uses a CAS loop to handle concurrent updates.
    ///
    /// Returns `true` if the request is within the limit, `false` if over limit.
    pub fn increment(&self, hits: u32) -> bool {
        let current_epoch = self.current_epoch();

        loop {
            let state = self.state.load(Ordering::Acquire);
            let (stored_epoch, count) = Self::unpack(state);

            let (new_epoch, new_count) = if stored_epoch == current_epoch {
                // Same window, add to existing count
                (current_epoch, count.saturating_add(hits))
            } else {
                // Window has rolled over, start fresh
                (current_epoch, hits)
            };

            let new_state = Self::pack(new_epoch, new_count);

            match self.state.compare_exchange_weak(
                state,
                new_state,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return (new_count as u64) <= self.limit,
                Err(_) => continue, // CAS failed, retry
            }
        }
    }

    /// Check if adding hits would exceed the limit without incrementing.
    pub fn would_exceed(&self, hits: u32) -> bool {
        let current_epoch = self.current_epoch();
        let state = self.state.load(Ordering::Acquire);
        let (stored_epoch, count) = Self::unpack(state);

        let effective_count = if stored_epoch == current_epoch {
            count as u64
        } else {
            0 // Window has rolled over
        };

        effective_count + hits as u64 > self.limit
    }

    /// Get the current count.
    ///
    /// Returns 0 if the window has rolled over since the last update.
    pub fn current_count(&self) -> u64 {
        let current_epoch = self.current_epoch();
        let state = self.state.load(Ordering::Acquire);
        let (stored_epoch, count) = Self::unpack(state);

        if stored_epoch == current_epoch {
            count as u64
        } else {
            0 // Window has rolled over
        }
    }

    /// Get the remaining quota.
    pub fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.current_count())
    }

    /// Get the limit for this counter.
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// Get the time window for this counter.
    pub fn window(&self) -> TimeWindow {
        self.window
    }

    /// Get the duration until the current window resets.
    pub fn duration_until_reset(&self) -> Duration {
        let elapsed = self.epoch_start.elapsed();
        let window_duration = self.window.duration();
        let elapsed_in_window = Duration::from_nanos(
            (elapsed.as_nanos() as u64) % (window_duration.as_nanos() as u64)
        );

        window_duration - elapsed_in_window
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_window_duration() {
        assert_eq!(TimeWindow::Second.duration(), Duration::from_secs(1));
        assert_eq!(TimeWindow::Minute.duration(), Duration::from_secs(60));
        assert_eq!(TimeWindow::Hour.duration(), Duration::from_secs(3600));
        assert_eq!(TimeWindow::Day.duration(), Duration::from_secs(86400));
    }

    #[test]
    fn test_counter_increment_within_limit() {
        let counter = RateLimitCounter::new(10, TimeWindow::Second);

        assert!(counter.increment(1));
        assert_eq!(counter.current_count(), 1);
        assert_eq!(counter.remaining(), 9);
    }

    #[test]
    fn test_counter_increment_exceeds_limit() {
        let counter = RateLimitCounter::new(5, TimeWindow::Second);

        for _ in 0..5 {
            assert!(counter.increment(1));
        }

        // The 6th request should be rejected
        assert!(!counter.increment(1));
    }

    #[test]
    fn test_counter_would_exceed() {
        let counter = RateLimitCounter::new(10, TimeWindow::Second);

        counter.increment(8);

        assert!(!counter.would_exceed(2)); // 8 + 2 = 10, within limit
        assert!(counter.would_exceed(3));  // 8 + 3 = 11, exceeds limit
    }

    #[test]
    fn test_counter_multi_hit_increment() {
        let counter = RateLimitCounter::new(10, TimeWindow::Second);

        assert!(counter.increment(5));
        assert_eq!(counter.current_count(), 5);
        assert_eq!(counter.remaining(), 5);
    }
}
