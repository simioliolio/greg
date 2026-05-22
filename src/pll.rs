use std::time::{Duration, Instant};

use crate::beat_confirmation::ConfirmedBeat;
use crate::clock::ClockState;
use crate::state::{SharedState, SystemState};

const BPM_CONVERGENCE_THRESHOLD: f64 = 2.0;
const PLL_MATCH_WINDOW: Duration = Duration::from_millis(250);
const RATE_NUDGE: f64 = 1.0;
const TAP_CONSTRAINT_RANGE: f64 = 5.0;
const PHASE_CORRECTION_THRESHOLD: Duration = Duration::from_millis(5);
const PHASE_CORRECTION_OFFSET: f64 = 2.0;
const MIN_BEATS_FOR_BPM: usize = 4;
const MAX_BEAT_AGE: Duration = Duration::from_secs(3);

pub struct Pll {
    tap_bpm: Option<f64>,
}

impl Pll {
    pub fn new() -> Self {
        Self { tap_bpm: None }
    }

    pub fn set_tap_bpm(&mut self, bpm: f64) {
        self.tap_bpm = Some(bpm);
    }

    pub fn update(
        &mut self,
        clock_beats: &[Instant],
        confirmed: &[ConfirmedBeat],
        system_state: &SharedState,
        clock_state: &ClockState,
    ) {
        let now = Instant::now();
        let state = system_state.get();

        let detected_bpm = self.detected_bpm(confirmed, now);

        // State transitions based on detected BPM
        match state {
            SystemState::Idle => {
                if detected_bpm.is_some() {
                    system_state.set(SystemState::RateLocking);
                }
            }
            SystemState::RateLocking => {
                if let Some(dbpm) = detected_bpm
                    && (clock_state.base_bpm() - dbpm).abs() < BPM_CONVERGENCE_THRESHOLD
                {
                    self.tap_bpm = None;
                    system_state.set(SystemState::Locked);
                }
            }
            SystemState::Locked => {
                if let Some(dbpm) = detected_bpm
                    && (clock_state.base_bpm() - dbpm).abs() > BPM_CONVERGENCE_THRESHOLD
                {
                    clock_state.set_temporary_offset(0.0);
                    system_state.set(SystemState::RateLocking);
                }
            }
        }

        let state = system_state.get();

        // Rate Correction (active in RateLocking and Locked)
        if matches!(state, SystemState::RateLocking | SystemState::Locked)
            && let Some(mut dbpm) = detected_bpm
        {
            if state == SystemState::RateLocking
                && let Some(tap) = self.tap_bpm
            {
                dbpm = dbpm.clamp(tap - TAP_CONSTRAINT_RANGE, tap + TAP_CONSTRAINT_RANGE);
            }

            let current = clock_state.base_bpm();
            let diff = dbpm - current;
            if diff.abs() > 0.01 {
                let nudge = diff.signum() * RATE_NUDGE.min(diff.abs());
                clock_state.set_base_bpm(current + nudge);
            }
        }

        // Phase Correction (active only in Locked)
        if state == SystemState::Locked
            && let Some(avg_offset) = self.pll_comparison(clock_beats, confirmed)
        {
            if avg_offset.unsigned_abs() > PHASE_CORRECTION_THRESHOLD {
                let sign = if avg_offset.is_positive() { 1.0 } else { -1.0 };
                clock_state.set_temporary_offset(sign * PHASE_CORRECTION_OFFSET);
            } else {
                clock_state.set_temporary_offset(0.0);
            }
        }
    }

    fn detected_bpm(&self, confirmed: &[ConfirmedBeat], now: Instant) -> Option<f64> {
        let recent: Vec<&ConfirmedBeat> = confirmed
            .iter()
            .filter(|b| now.duration_since(b.timestamp) < MAX_BEAT_AGE)
            .collect();

        if recent.len() < MIN_BEATS_FOR_BPM {
            return None;
        }

        let mut intervals: Vec<f64> = recent
            .windows(2)
            .map(|w| w[1].timestamp.duration_since(w[0].timestamp).as_secs_f64())
            .filter(|&i| i > 0.1 && i < 2.0)
            .collect();

        if intervals.len() < MIN_BEATS_FOR_BPM - 1 {
            return None;
        }

        intervals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = intervals[intervals.len() / 2];
        Some(60.0 / median)
    }

    fn pll_comparison(
        &self,
        clock_beats: &[Instant],
        confirmed: &[ConfirmedBeat],
    ) -> Option<SignedDuration> {
        if clock_beats.len() < 2 || confirmed.is_empty() {
            return None;
        }

        let last_two = &clock_beats[clock_beats.len().saturating_sub(2)..];
        let mut offsets = Vec::new();

        for &clock_beat in last_two {
            if let Some(nearest) = find_nearest_confirmed(clock_beat, confirmed, PLL_MATCH_WINDOW) {
                offsets.push(signed_diff(nearest.timestamp, clock_beat));
            }
        }

        if offsets.is_empty() {
            return None;
        }

        let sum: f64 = offsets.iter().map(|d| d.as_secs_f64()).sum();
        let avg_secs = sum / offsets.len() as f64;
        Some(SignedDuration::from_secs_f64(avg_secs))
    }
}

fn find_nearest_confirmed(
    target: Instant,
    confirmed: &[ConfirmedBeat],
    window: Duration,
) -> Option<&ConfirmedBeat> {
    confirmed
        .iter()
        .filter(|b| {
            let diff = if b.timestamp >= target {
                b.timestamp.duration_since(target)
            } else {
                target.duration_since(b.timestamp)
            };
            diff <= window
        })
        .min_by_key(|b| {
            if b.timestamp >= target {
                b.timestamp.duration_since(target)
            } else {
                target.duration_since(b.timestamp)
            }
        })
}

#[derive(Debug, Clone, Copy)]
struct SignedDuration {
    nanos: i128,
}

impl SignedDuration {
    fn from_secs_f64(secs: f64) -> Self {
        Self {
            nanos: (secs * 1_000_000_000.0) as i128,
        }
    }

    fn as_secs_f64(self) -> f64 {
        self.nanos as f64 / 1_000_000_000.0
    }

    fn unsigned_abs(self) -> Duration {
        Duration::from_nanos(self.nanos.unsigned_abs() as u64)
    }

    fn is_positive(self) -> bool {
        self.nanos > 0
    }
}

fn signed_diff(a: Instant, b: Instant) -> SignedDuration {
    if a >= b {
        SignedDuration::from_secs_f64(a.duration_since(b).as_secs_f64())
    } else {
        SignedDuration::from_secs_f64(-(b.duration_since(a).as_secs_f64()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_confirmed(base: Instant, offsets_ms: &[u64]) -> Vec<ConfirmedBeat> {
        offsets_ms
            .iter()
            .map(|&ms| ConfirmedBeat {
                timestamp: base + Duration::from_millis(ms),
            })
            .collect()
    }

    #[test]
    fn detected_bpm_from_regular_beats() {
        let pll = Pll::new();
        let base = Instant::now();
        let confirmed = make_confirmed(base, &[0, 500, 1000, 1500, 2000]);
        let bpm = pll.detected_bpm(&confirmed, base + Duration::from_millis(2100));
        assert!(bpm.is_some());
        let bpm = bpm.unwrap();
        assert!(
            (bpm - 120.0).abs() < 1.0,
            "expected ~120 BPM, got {bpm}"
        );
    }

    #[test]
    fn detected_bpm_insufficient_beats() {
        let pll = Pll::new();
        let base = Instant::now();
        let confirmed = make_confirmed(base, &[0, 500]);
        let bpm = pll.detected_bpm(&confirmed, base + Duration::from_millis(600));
        assert!(bpm.is_none());
    }

    #[test]
    fn rate_correction_nudges_toward_detected() {
        let mut pll = Pll::new();
        let base = Instant::now();
        let confirmed = make_confirmed(base, &[0, 500, 1000, 1500, 2000]);
        let clock_state = ClockState::new(110.0);
        let system_state = SharedState::new();
        system_state.set(SystemState::RateLocking);

        // PLL update with clock beats
        let clock_beats = vec![
            base + Duration::from_millis(1000),
            base + Duration::from_millis(1500),
        ];

        // Run a few updates
        for _ in 0..5 {
            pll.update(&clock_beats, &confirmed, &system_state, &clock_state);
        }

        // base_bpm should have nudged toward 120
        assert!(
            clock_state.base_bpm() > 110.0,
            "expected BPM to increase from 110, got {}",
            clock_state.base_bpm()
        );
    }

    #[test]
    fn state_transitions_idle_to_rate_locking() {
        let mut pll = Pll::new();
        let base = Instant::now();
        let confirmed = make_confirmed(base, &[0, 500, 1000, 1500, 2000]);
        let clock_state = ClockState::new(120.0);
        let system_state = SharedState::new();

        assert_eq!(system_state.get(), SystemState::Idle);

        pll.update(&[], &confirmed, &system_state, &clock_state);

        assert_eq!(system_state.get(), SystemState::RateLocking);
    }

    #[test]
    fn state_transitions_rate_locking_to_locked() {
        let mut pll = Pll::new();
        let base = Instant::now();
        let confirmed = make_confirmed(base, &[0, 500, 1000, 1500, 2000]);
        let clock_state = ClockState::new(120.0);
        let system_state = SharedState::new();
        system_state.set(SystemState::RateLocking);

        let clock_beats = vec![
            base + Duration::from_millis(1000),
            base + Duration::from_millis(1500),
        ];

        pll.update(&clock_beats, &confirmed, &system_state, &clock_state);

        // BPM already matches — should transition to Locked
        assert_eq!(system_state.get(), SystemState::Locked);
    }

    #[test]
    fn tap_constraint_clamps_detected_bpm() {
        let mut pll = Pll::new();
        pll.set_tap_bpm(100.0);

        let base = Instant::now();
        // Confirmed beats at 120 BPM
        let confirmed = make_confirmed(base, &[0, 500, 1000, 1500, 2000]);
        let clock_state = ClockState::new(100.0);
        let system_state = SharedState::new();
        system_state.set(SystemState::RateLocking);

        let clock_beats = vec![base + Duration::from_millis(1000)];

        for _ in 0..20 {
            pll.update(&clock_beats, &confirmed, &system_state, &clock_state);
        }

        // Should be clamped to tap_bpm + 5 = 105, not 120
        assert!(
            clock_state.base_bpm() <= 106.0,
            "expected BPM clamped to ~105, got {}",
            clock_state.base_bpm()
        );
    }

    #[test]
    fn phase_correction_activates_in_locked() {
        let mut pll = Pll::new();
        let base = Instant::now();

        // Confirmed beats slightly ahead of clock beats
        let confirmed = make_confirmed(base, &[10, 510, 1010, 1510, 2010]);
        let clock_state = ClockState::new(120.0);
        let system_state = SharedState::new();
        system_state.set(SystemState::Locked);

        let clock_beats = vec![
            base + Duration::from_millis(1000),
            base + Duration::from_millis(1500),
        ];

        pll.update(&clock_beats, &confirmed, &system_state, &clock_state);

        // With 10ms offset, phase correction should activate
        let offset = clock_state.temporary_offset();
        assert!(
            offset.abs() > 0.0,
            "expected nonzero temporary offset, got {offset}"
        );
    }
}
