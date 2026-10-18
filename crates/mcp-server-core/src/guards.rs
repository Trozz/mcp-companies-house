use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv6Addr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Failures across all clients within one window that block every login.
/// Trade-off: during an active distributed attack the owner is locked out
/// too, but the total guessing rate stays bounded.
const GLOBAL_MAX_FAILURES: usize = 30;
const MAX_TRACKED_KEYS: usize = 10_000;
const SWEEP_INTERVAL: Duration = Duration::from_secs(1);

pub struct LoginLimiter {
    max: usize,
    window: Duration,
    state: Mutex<LimiterState>,
}

struct LimiterState {
    per_key: HashMap<IpAddr, Vec<Instant>>,
    global: VecDeque<Instant>,
    last_sweep: Instant,
}

fn limiter_key(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V6(v6) => {
            let s = v6.segments();
            IpAddr::V6(Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
        }
        v4 => v4,
    }
}

impl LimiterState {
    fn prune(&mut self, now: Instant, window: Duration) {
        while self
            .global
            .front()
            .is_some_and(|t| now.duration_since(*t) >= window)
        {
            self.global.pop_front();
        }
        if now.duration_since(self.last_sweep) < SWEEP_INTERVAL {
            return;
        }
        self.last_sweep = now;
        self.per_key.retain(|_, times| {
            times.retain(|t| now.duration_since(*t) < window);
            !times.is_empty()
        });
    }
}

impl LoginLimiter {
    pub fn new(max: usize, window: Duration) -> Self {
        Self {
            max,
            window,
            state: Mutex::new(LimiterState {
                per_key: HashMap::new(),
                global: VecDeque::new(),
                last_sweep: Instant::now(),
            }),
        }
    }

    pub fn is_blocked(&self, ip: IpAddr) -> bool {
        let mut state = self.state.lock().expect("limiter lock");
        let now = Instant::now();
        state.prune(now, self.window);
        if state.global.len() >= GLOBAL_MAX_FAILURES {
            return true;
        }
        let Some(times) = state.per_key.get_mut(&limiter_key(ip)) else {
            return false;
        };
        times.retain(|t| now.duration_since(*t) < self.window);
        times.len() >= self.max
    }

    pub fn record_failure(&self, ip: IpAddr) {
        let mut state = self.state.lock().expect("limiter lock");
        let now = Instant::now();
        state.prune(now, self.window);
        state.global.push_back(now);
        let key = limiter_key(ip);
        if state.per_key.len() >= MAX_TRACKED_KEYS && !state.per_key.contains_key(&key) {
            return;
        }
        state.per_key.entry(key).or_default().push(now);
    }
}

pub struct ReplayGuard {
    used: Mutex<HashMap<String, u64>>,
}

impl ReplayGuard {
    pub fn new() -> Self {
        Self {
            used: Mutex::new(HashMap::new()),
        }
    }

    pub fn first_use(&self, jti: &str, exp_unix: u64) -> bool {
        let now = jsonwebtoken::get_current_timestamp();
        let mut used = self.used.lock().expect("replay lock");
        used.retain(|_, exp| *exp >= now);
        used.insert(jti.to_string(), exp_unix).is_none()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.used.lock().unwrap().len()
    }
}

impl Default for ReplayGuard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_blocks_after_max_failures() {
        let l = LoginLimiter::new(3, Duration::from_secs(60));
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let other: IpAddr = "10.0.0.2".parse().unwrap();
        for _ in 0..3 {
            assert!(!l.is_blocked(ip));
            l.record_failure(ip);
        }
        assert!(l.is_blocked(ip));
        assert!(!l.is_blocked(other));
    }

    #[test]
    fn limiter_forgets_after_window() {
        let l = LoginLimiter::new(1, Duration::from_millis(20));
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        l.record_failure(ip);
        assert!(l.is_blocked(ip));
        std::thread::sleep(Duration::from_millis(40));
        assert!(!l.is_blocked(ip));
    }

    #[test]
    fn ipv6_addresses_in_one_slash_64_share_a_bucket() {
        let l = LoginLimiter::new(2, Duration::from_secs(60));
        let a: IpAddr = "2001:db8:1:2::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:ffff:ffff:ffff:ffff".parse().unwrap();
        let other: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        l.record_failure(a);
        l.record_failure(b);
        assert!(l.is_blocked(a));
        assert!(l.is_blocked(b));
        assert!(!l.is_blocked(other));
    }

    #[test]
    fn ipv4_mapped_ipv6_is_keyed_as_ipv4() {
        let l = LoginLimiter::new(1, Duration::from_secs(60));
        l.record_failure("::ffff:10.0.0.1".parse().unwrap());
        assert!(l.is_blocked("10.0.0.1".parse().unwrap()));
        assert!(!l.is_blocked("::ffff:10.0.0.2".parse().unwrap()));
    }

    #[test]
    fn global_ceiling_blocks_a_fresh_ip() {
        let l = LoginLimiter::new(5, Duration::from_secs(60));
        for i in 0..GLOBAL_MAX_FAILURES {
            let ip = IpAddr::from([10, 0, (i / 256) as u8, (i % 256) as u8]);
            assert!(!l.is_blocked(ip));
            l.record_failure(ip);
        }
        assert!(l.is_blocked("192.0.2.1".parse().unwrap()));
    }

    #[test]
    fn tracked_keys_are_capped_but_still_count_globally() {
        let l = LoginLimiter::new(1, Duration::from_secs(60));
        for i in 0..MAX_TRACKED_KEYS as u32 + 5 {
            let ip = IpAddr::from(i.to_be_bytes());
            l.record_failure(ip);
        }
        let state = l.state.lock().unwrap();
        assert_eq!(state.per_key.len(), MAX_TRACKED_KEYS);
        assert_eq!(state.global.len(), MAX_TRACKED_KEYS + 5);
    }

    #[test]
    fn replay_guard_allows_first_use_only() {
        let g = ReplayGuard::new();
        let exp = jsonwebtoken::get_current_timestamp() + 60;
        assert!(g.first_use("jti-1", exp));
        assert!(!g.first_use("jti-1", exp));
        assert!(g.first_use("jti-2", exp));
    }

    #[test]
    fn replay_guard_prunes_expired() {
        let g = ReplayGuard::new();
        let past = jsonwebtoken::get_current_timestamp() - 10;
        assert!(g.first_use("old", past));
        g.first_use("trigger-prune", past + 1000);
        assert_eq!(g.len(), 1);
    }
}
