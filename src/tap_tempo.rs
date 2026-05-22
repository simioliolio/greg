use std::time::{Duration, Instant};

const TAP_TIMEOUT: Duration = Duration::from_secs(3);
const MIN_TAPS: usize = 3;

pub struct TapResult {
    pub bpm: f64,
}

pub struct TapTempo {
    timestamps: Vec<Instant>,
}

impl TapTempo {
    pub fn new() -> Self {
        Self {
            timestamps: Vec::new(),
        }
    }

    pub fn tap(&mut self, now: Instant) -> Option<TapResult> {
        if let Some(&last) = self.timestamps.last()
            && now.duration_since(last) > TAP_TIMEOUT
        {
            self.timestamps.clear();
        }

        self.timestamps.push(now);

        if self.timestamps.len() < MIN_TAPS {
            return None;
        }

        let interval = if self.timestamps.len() == MIN_TAPS {
            let total: Duration = self
                .timestamps
                .last()
                .unwrap()
                .duration_since(*self.timestamps.first().unwrap());
            total.as_secs_f64() / (self.timestamps.len() - 1) as f64
        } else {
            let len = self.timestamps.len();
            self.timestamps[len - 1]
                .duration_since(self.timestamps[len - 2])
                .as_secs_f64()
        };

        if interval > 0.0 {
            Some(TapResult {
                bpm: 60.0 / interval,
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_taps_at_120bpm() {
        let mut tt = TapTempo::new();
        let base = Instant::now();
        let interval = Duration::from_millis(500);

        assert!(tt.tap(base).is_none());
        assert!(tt.tap(base + interval).is_none());

        let result = tt.tap(base + interval * 2);
        assert!(result.is_some());
        let bpm = result.unwrap().bpm;
        assert!(
            (bpm - 120.0).abs() < 1.0,
            "expected ~120 BPM, got {bpm}"
        );
    }

    #[test]
    fn two_taps_not_enough() {
        let mut tt = TapTempo::new();
        let base = Instant::now();
        assert!(tt.tap(base).is_none());
        assert!(tt.tap(base + Duration::from_millis(500)).is_none());
    }

    #[test]
    fn timeout_resets() {
        let mut tt = TapTempo::new();
        let base = Instant::now();
        let interval = Duration::from_millis(500);

        tt.tap(base);
        tt.tap(base + interval);

        // Gap > 3 seconds
        assert!(tt.tap(base + Duration::from_secs(5)).is_none());
        // Need 2 more taps to reach 3
        assert!(
            tt.tap(base + Duration::from_secs(5) + interval)
                .is_none()
        );
        let result = tt.tap(base + Duration::from_secs(5) + interval * 2);
        assert!(result.is_some());
    }

    #[test]
    fn fourth_tap_updates_bpm() {
        let mut tt = TapTempo::new();
        let base = Instant::now();
        let slow = Duration::from_millis(500);
        let fast = Duration::from_millis(400);

        tt.tap(base);
        tt.tap(base + slow);
        let r1 = tt.tap(base + slow * 2).unwrap();
        assert!((r1.bpm - 120.0).abs() < 1.0);

        // 4th tap at faster interval
        let r2 = tt.tap(base + slow * 2 + fast).unwrap();
        assert!(
            (r2.bpm - 150.0).abs() < 1.0,
            "expected ~150 BPM, got {}",
            r2.bpm
        );
    }
}
