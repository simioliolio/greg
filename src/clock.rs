use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;

use crate::beat_confirmation::ConfirmedBeat;
use crate::input::TapEvent;
use crate::pll::Pll;
use crate::state::{SharedState, SystemState};
use crate::time_source::TimeSource;

const PULSES_PER_QUARTER_NOTE: u64 = 24;
const BEAT_TIMESTAMP_CAPACITY: usize = 8;

pub struct ClockState {
    base_bpm_bits: AtomicU64,
    temporary_offset_bits: AtomicU64,
}

impl ClockState {
    pub fn new(initial_bpm: f64) -> Self {
        Self {
            base_bpm_bits: AtomicU64::new(initial_bpm.to_bits()),
            temporary_offset_bits: AtomicU64::new(0.0_f64.to_bits()),
        }
    }

    pub fn base_bpm(&self) -> f64 {
        f64::from_bits(self.base_bpm_bits.load(Ordering::Relaxed))
    }

    pub fn set_base_bpm(&self, bpm: f64) {
        self.base_bpm_bits.store(bpm.to_bits(), Ordering::Relaxed);
    }

    pub fn temporary_offset(&self) -> f64 {
        f64::from_bits(self.temporary_offset_bits.load(Ordering::Relaxed))
    }

    pub fn set_temporary_offset(&self, offset: f64) {
        self.temporary_offset_bits
            .store(offset.to_bits(), Ordering::Relaxed);
    }

    pub fn effective_bpm(&self) -> f64 {
        self.base_bpm() + self.temporary_offset()
    }
}

pub fn pulse_interval(bpm: f64) -> Duration {
    let secs = 60.0 / (bpm * PULSES_PER_QUARTER_NOTE as f64);
    Duration::from_secs_f64(secs)
}

pub fn run<F>(
    clock_state: Arc<ClockState>,
    system_state: Arc<SharedState>,
    time_source: Arc<dyn TimeSource>,
    tap_rx: Receiver<TapEvent>,
    confirmed_beats: Arc<Mutex<Vec<ConfirmedBeat>>>,
    mut on_pulse: F,
) where
    F: FnMut(bool) + Send,
{
    let mut pulse_count: u64 = 0;
    let mut beat_timestamps: VecDeque<Instant> = VecDeque::with_capacity(BEAT_TIMESTAMP_CAPACITY);
    let mut next_pulse = time_source.now();
    let mut pll = Pll::new();

    loop {
        time_source.sleep_until(next_pulse);

        // Check for tap tempo events (non-blocking)
        if let Ok(tap) = tap_rx.try_recv() {
            clock_state.set_base_bpm(tap.bpm);
            clock_state.set_temporary_offset(0.0);
            pll.set_tap_bpm(tap.bpm);
            system_state.set(SystemState::RateLocking);
            // Phase reset: next beat starts NOW (ADR-0001)
            pulse_count = 0;
            next_pulse = time_source.now();
            beat_timestamps.clear();
            beat_timestamps.push_back(next_pulse);
            eprintln!("[clock] tap tempo: {:.1} BPM, phase reset", tap.bpm);
            continue;
        }

        let is_beat = pulse_count.is_multiple_of(PULSES_PER_QUARTER_NOTE);

        if is_beat {
            let now = time_source.now();
            if beat_timestamps.len() >= BEAT_TIMESTAMP_CAPACITY {
                beat_timestamps.pop_front();
            }
            beat_timestamps.push_back(now);

            // Run PLL on each beat
            let beats_snapshot: Vec<Instant> = beat_timestamps.iter().copied().collect();
            let confirmed_snapshot = confirmed_beats
                .lock()
                .map(|b| b.clone())
                .unwrap_or_default();
            pll.update(
                &beats_snapshot,
                &confirmed_snapshot,
                &system_state,
                &clock_state,
            );
        }

        let gate_open = system_state.midi_active();
        on_pulse(gate_open);

        pulse_count += 1;
        let interval = pulse_interval(clock_state.effective_bpm());
        next_pulse += interval;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time_source::RealTimeSource;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn pulse_interval_120bpm() {
        let interval = pulse_interval(120.0);
        let expected = Duration::from_secs_f64(60.0 / (120.0 * 24.0));
        let diff = if interval > expected {
            interval - expected
        } else {
            expected - interval
        };
        assert!(diff < Duration::from_micros(1));
    }

    #[test]
    fn pulse_interval_various_bpms() {
        for bpm in [60.0, 90.0, 120.0, 140.0, 180.0] {
            let interval = pulse_interval(bpm);
            let expected_secs = 60.0 / (bpm * 24.0);
            let diff = (interval.as_secs_f64() - expected_secs).abs();
            assert!(diff < 1e-9, "BPM {bpm}: interval off by {diff}s");
        }
    }

    #[test]
    fn clock_state_bpm_operations() {
        let state = ClockState::new(120.0);
        assert!((state.base_bpm() - 120.0).abs() < f64::EPSILON);
        assert!((state.temporary_offset() - 0.0).abs() < f64::EPSILON);
        assert!((state.effective_bpm() - 120.0).abs() < f64::EPSILON);

        state.set_base_bpm(130.0);
        state.set_temporary_offset(2.0);
        assert!((state.effective_bpm() - 132.0).abs() < f64::EPSILON);
    }

    #[test]
    fn clock_emits_pulses_with_gate() {
        let clock_state = Arc::new(ClockState::new(120.0));
        let system_state = Arc::new(SharedState::new());
        system_state.set(SystemState::RateLocking);

        let ts: Arc<dyn TimeSource> = Arc::new(RealTimeSource);
        let (tx, rx) = mpsc::channel();
        let cs_clone = Arc::clone(&clock_state);
        let ss_clone = Arc::clone(&system_state);
        let ts_clone = Arc::clone(&ts);
        let confirmed = Arc::new(Mutex::new(Vec::new()));

        let (_tap_tx, tap_rx) = crossbeam_channel::unbounded();
        let handle = thread::spawn(move || {
            let mut count = 0u32;
            run(cs_clone, ss_clone, ts_clone, tap_rx, confirmed, |gate_open| {
                let _ = tx.send(gate_open);
                count += 1;
                if count >= 48 {
                    std::process::exit(0);
                }
            });
        });

        let mut gated_pulses = 0;
        let start = Instant::now();
        let timeout = Duration::from_secs(3);

        while start.elapsed() < timeout {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(true) => gated_pulses += 1,
                Ok(false) => {}
                Err(_) => break,
            }
            if gated_pulses >= 48 {
                break;
            }
        }

        assert!(
            gated_pulses >= 24,
            "expected at least 24 gated pulses, got {gated_pulses}"
        );

        drop(handle);
    }
}
