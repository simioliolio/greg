use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

pub trait TimeSource: Send + Sync {
    fn now(&self) -> Instant;
    fn sleep_until(&self, deadline: Instant);
}

pub struct RealTimeSource;

impl TimeSource for RealTimeSource {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep_until(&self, deadline: Instant) {
        let now = Instant::now();
        if deadline > now {
            std::thread::sleep(deadline - now);
        }
    }
}

pub struct SyntheticTimeSource {
    current: Mutex<Instant>,
    advanced: Condvar,
}

impl SyntheticTimeSource {
    pub fn new(base: Instant) -> Self {
        Self {
            current: Mutex::new(base),
            advanced: Condvar::new(),
        }
    }

    pub fn advance_by(&self, duration: Duration) {
        let mut current = self.current.lock().unwrap();
        *current += duration;
        self.advanced.notify_all();
    }
}

impl TimeSource for SyntheticTimeSource {
    fn now(&self) -> Instant {
        *self.current.lock().unwrap()
    }

    fn sleep_until(&self, deadline: Instant) {
        let mut current = self.current.lock().unwrap();
        while *current < deadline {
            current = self.advanced.wait(current).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_returns_increasing_values() {
        let ts = RealTimeSource;
        let t1 = ts.now();
        std::thread::sleep(Duration::from_millis(1));
        let t2 = ts.now();
        assert!(t2 > t1);
    }

    #[test]
    fn sleep_until_past_returns_immediately() {
        let ts = RealTimeSource;
        let past = ts.now() - Duration::from_secs(1);
        let before = Instant::now();
        ts.sleep_until(past);
        let elapsed = before.elapsed();
        assert!(elapsed < Duration::from_millis(10));
    }

    #[test]
    fn sleep_until_future_sleeps() {
        let ts = RealTimeSource;
        let target = ts.now() + Duration::from_millis(50);
        ts.sleep_until(target);
        assert!(Instant::now() >= target);
    }

    #[test]
    fn synthetic_now_returns_base() {
        let base = Instant::now();
        let ts = SyntheticTimeSource::new(base);
        assert_eq!(ts.now(), base);
    }

    #[test]
    fn synthetic_advance_by() {
        let base = Instant::now();
        let ts = SyntheticTimeSource::new(base);
        ts.advance_by(Duration::from_secs(5));
        assert_eq!(ts.now(), base + Duration::from_secs(5));
    }

    #[test]
    fn synthetic_sleep_until_past_returns_immediately() {
        let base = Instant::now();
        let ts = SyntheticTimeSource::new(base + Duration::from_secs(10));
        let before = Instant::now();
        ts.sleep_until(base);
        assert!(before.elapsed() < Duration::from_millis(10));
    }

    #[test]
    fn synthetic_sleep_until_future_blocks_then_wakes() {
        use std::sync::Arc;

        let base = Instant::now();
        let ts = Arc::new(SyntheticTimeSource::new(base));
        let ts_clone = Arc::clone(&ts);
        let deadline = base + Duration::from_secs(5);

        let handle = std::thread::spawn(move || {
            ts_clone.sleep_until(deadline);
            ts_clone.now()
        });

        std::thread::sleep(Duration::from_millis(10));
        ts.advance_by(Duration::from_secs(5));

        let woke_at = handle.join().unwrap();
        assert!(woke_at >= deadline);
    }
}
