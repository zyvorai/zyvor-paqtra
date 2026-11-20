// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Failed-login throttle.
//
// After MAX_FAILURES failed logins from one client IP inside WINDOW, further
// attempts from that IP are refused until the window ends, whether or not the
// credentials are right. This slows password guessing beyond what the general
// mutating-request rate limit (10 requests/second per IP) allows. It is per IP,
// not per account, on purpose: a per-account lockout would let anyone lock the
// real admin out. Guessing spread across many IPs is not covered.

use dashmap::DashMap;
use std::net::IpAddr;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub const MAX_FAILURES: u32 = 10;
pub const WINDOW: Duration = Duration::from_secs(15 * 60);
/// Above this many tracked IPs, expired entries are dropped on the next write.
const PRUNE_ABOVE: usize = 10_000;

#[derive(Default)]
pub struct LoginGuard {
    /// IP -> (failures in the current window, window start)
    failures: DashMap<IpAddr, (u32, Instant)>,
}

impl LoginGuard {
    /// Ok if the IP may try, or Err(seconds until it may).
    pub fn check(&self, ip: IpAddr, now: Instant) -> Result<(), u64> {
        let Some(entry) = self.failures.get(&ip) else {
            return Ok(());
        };
        let (count, start) = *entry;
        drop(entry);
        let elapsed = now.saturating_duration_since(start);
        if elapsed >= WINDOW {
            self.failures.remove(&ip);
            Ok(())
        } else if count >= MAX_FAILURES {
            Err((WINDOW - elapsed).as_secs().max(1))
        } else {
            Ok(())
        }
    }

    pub fn record_failure(&self, ip: IpAddr, now: Instant) {
        if self.failures.len() > PRUNE_ABOVE {
            self.failures
                .retain(|_, (_, start)| now.saturating_duration_since(*start) < WINDOW);
        }
        let mut entry = self.failures.entry(ip).or_insert((0, now));
        if now.saturating_duration_since(entry.1) >= WINDOW {
            *entry = (0, now);
        }
        entry.0 += 1;
    }

    /// Forget an IP after a successful login.
    pub fn clear(&self, ip: IpAddr) {
        self.failures.remove(&ip);
    }
}

pub fn guard() -> &'static LoginGuard {
    static GUARD: OnceLock<LoginGuard> = OnceLock::new();
    GUARD.get_or_init(LoginGuard::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(n: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, n])
    }

    #[test]
    fn allows_up_to_the_limit_then_blocks() {
        let g = LoginGuard::default();
        let t = Instant::now();
        for _ in 0..MAX_FAILURES {
            assert_eq!(g.check(ip(1), t), Ok(()));
            g.record_failure(ip(1), t);
        }
        let retry = g.check(ip(1), t).unwrap_err();
        assert!(retry > 0 && retry <= WINDOW.as_secs(), "{retry}");
    }

    #[test]
    fn one_ip_being_blocked_does_not_affect_another() {
        let g = LoginGuard::default();
        let t = Instant::now();
        for _ in 0..MAX_FAILURES {
            g.record_failure(ip(1), t);
        }
        assert!(g.check(ip(1), t).is_err());
        assert_eq!(g.check(ip(2), t), Ok(()));
    }

    #[test]
    fn block_ends_when_the_window_does() {
        let g = LoginGuard::default();
        let t = Instant::now();
        for _ in 0..MAX_FAILURES {
            g.record_failure(ip(1), t);
        }
        assert!(g.check(ip(1), t + WINDOW - Duration::from_secs(1)).is_err());
        assert_eq!(g.check(ip(1), t + WINDOW), Ok(()));
        // And the counter starts fresh afterwards.
        g.record_failure(ip(1), t + WINDOW);
        assert_eq!(g.check(ip(1), t + WINDOW), Ok(()));
    }

    #[test]
    fn success_clears_the_count() {
        let g = LoginGuard::default();
        let t = Instant::now();
        for _ in 0..MAX_FAILURES - 1 {
            g.record_failure(ip(1), t);
        }
        g.clear(ip(1));
        for _ in 0..MAX_FAILURES - 1 {
            g.record_failure(ip(1), t);
        }
        assert_eq!(
            g.check(ip(1), t),
            Ok(()),
            "9 + 9 failures split by a success is not a lockout"
        );
    }

    #[test]
    fn stale_failures_do_not_accumulate_across_windows() {
        let g = LoginGuard::default();
        let t = Instant::now();
        for _ in 0..MAX_FAILURES - 1 {
            g.record_failure(ip(1), t);
        }
        let later = t + WINDOW + Duration::from_secs(1);
        g.record_failure(ip(1), later);
        assert_eq!(g.check(ip(1), later), Ok(()));
    }
}
