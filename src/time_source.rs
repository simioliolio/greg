use std::time::Instant;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

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
}
