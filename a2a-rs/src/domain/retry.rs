//! Retry/backoff policy for resilient streaming subscriptions.
//!
//! [`RetryPolicy`] is a pure value object: it carries the knobs for exponential
//! backoff with jitter and computes the delay for a given attempt, with no I/O,
//! no clock, and no randomness source of its own. The impure parts — sleeping
//! and seeding the jitter — live in the transport adapter that consumes it
//! (`adapter::transport::subscribe_resilient`), keeping this type
//! domain-pure and unit-testable.

use std::time::Duration;

/// Exponential-backoff-with-jitter policy for reconnecting a dropped stream.
///
/// The delay before retry *n* (1-based) is `base_delay * 2^(n-1)`, capped at
/// `max_delay`, plus up to `jitter_ms` of seeded jitter (and re-capped at
/// `max_delay`). After `max_retries` consecutive failures the consumer gives up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Delay before the first retry; doubles each subsequent attempt.
    pub base_delay: Duration,
    /// Upper bound on any single delay.
    pub max_delay: Duration,
    /// Maximum consecutive failed attempts before giving up.
    pub max_retries: u32,
    /// Maximum jitter span in milliseconds added to each delay (`0` disables
    /// jitter, making delays deterministic).
    pub jitter_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(10),
            max_retries: 15,
            jitter_ms: 200,
        }
    }
}

impl RetryPolicy {
    /// A policy that never retries — the subscription fails on first disconnect.
    pub fn no_retry() -> Self {
        Self {
            max_retries: 0,
            ..Self::default()
        }
    }

    /// Compute the delay before `attempt` (1-based), folding `seed` into the
    /// jitter. Pure: identical `(attempt, seed)` always yields the same delay.
    pub fn backoff(&self, attempt: u32, seed: u64) -> Duration {
        let base_ms = self.base_delay.as_millis() as u64;
        let max_ms = self.max_delay.as_millis() as u64;

        // Exponential growth: base * 2^(attempt-1), saturating, then capped.
        let shift = attempt.saturating_sub(1).min(63);
        let factor = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
        let grown = base_ms.saturating_mul(factor).min(max_ms);

        let jitter = if self.jitter_ms == 0 {
            0
        } else {
            mix(seed) % self.jitter_ms
        };

        Duration::from_millis(grown.saturating_add(jitter).min(max_ms))
    }
}

/// Deterministic jitter mixer (a single SplitMix64-style round). Keeps jitter
/// dependency-free (no `rand`) while spreading reconnect storms across clients
/// with different seeds.
#[inline]
fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(jitter_ms: u64) -> RetryPolicy {
        RetryPolicy {
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            max_retries: 10,
            jitter_ms,
        }
    }

    #[test]
    fn grows_exponentially_without_jitter() {
        let p = policy(0);
        assert_eq!(p.backoff(1, 0), Duration::from_millis(100));
        assert_eq!(p.backoff(2, 0), Duration::from_millis(200));
        assert_eq!(p.backoff(3, 0), Duration::from_millis(400));
        assert_eq!(p.backoff(4, 0), Duration::from_millis(800));
    }

    #[test]
    fn caps_at_max_delay() {
        let p = policy(0);
        // 100ms * 2^20 is far past the 10s cap.
        assert_eq!(p.backoff(20, 12345), Duration::from_secs(10));
        // Huge attempt must not panic on overflow.
        assert_eq!(p.backoff(u32::MAX, 1), Duration::from_secs(10));
    }

    #[test]
    fn jitter_stays_within_span_and_is_deterministic() {
        let p = policy(200);
        for seed in 0..1000u64 {
            let d = p.backoff(1, seed).as_millis() as u64;
            // base 100ms + jitter in [0, 200)
            assert!(
                (100..300).contains(&d),
                "delay {d} out of range for seed {seed}"
            );
            // deterministic
            assert_eq!(p.backoff(1, seed), p.backoff(1, seed));
        }
    }

    #[test]
    fn jitter_varies_across_seeds() {
        let p = policy(200);
        let a = p.backoff(1, 1);
        let b = p.backoff(1, 2);
        assert_ne!(a, b, "different seeds should generally jitter differently");
    }
}
