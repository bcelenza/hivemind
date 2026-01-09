//! Rate limit counter implementation.

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
/// This counter is designed to be thread-safe and uses atomic operations
/// for lock-free updates.
pub struct RateLimitCounter {
    /// Current count of requests in this window
    count: AtomicU64,
    /// The limit for this counter
    limit: u64,
    /// Time window for this counter
    window: TimeWindow,
    /// When the current window started
    window_start: std::sync::Mutex<Instant>,
}

impl RateLimitCounter {
    /// Create a new rate limit counter.
    pub fn new(limit: u64, window: TimeWindow) -> Self {
        Self {
            count: AtomicU64::new(0),
            limit,
            window,
            window_start: std::sync::Mutex::new(Instant::now()),
        }
    }

    /// Increment the counter and check if the limit has been exceeded.
    ///
    /// Returns `true` if the request is within the limit, `false` if over limit.
    pub fn increment(&self, hits: u32) -> bool {
        self.maybe_reset_window();
        
        let new_count = self.count.fetch_add(hits as u64, Ordering::SeqCst) + hits as u64;
        new_count <= self.limit
    }

    /// Check if adding hits would exceed the limit without incrementing.
    pub fn would_exceed(&self, hits: u32) -> bool {
        self.maybe_reset_window();
        
        let current = self.count.load(Ordering::SeqCst);
        current + hits as u64 > self.limit
    }

    /// Get the current count.
    pub fn current_count(&self) -> u64 {
        self.maybe_reset_window();
        self.count.load(Ordering::SeqCst)
    }

    /// Get the remaining quota.
    pub fn remaining(&self) -> u64 {
        self.maybe_reset_window();
        let current = self.count.load(Ordering::SeqCst);
        self.limit.saturating_sub(current)
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
        let window_start = self.window_start.lock().unwrap();
        let elapsed = window_start.elapsed();
        let window_duration = self.window.duration();
        
        if elapsed >= window_duration {
            Duration::ZERO
        } else {
            window_duration - elapsed
        }
    }

    /// Reset the window if it has expired.
    fn maybe_reset_window(&self) {
        let mut window_start = self.window_start.lock().unwrap();
        let elapsed = window_start.elapsed();
        
        if elapsed >= self.window.duration() {
            // Reset the counter and window
            self.count.store(0, Ordering::SeqCst);
            *window_start = Instant::now();
        }
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
