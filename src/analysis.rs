use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::beat_confirmation::{BeatConfirmation, ConfirmedBeat};
use crate::beat_detector::BeatDetector;
use crate::ring_buffer::RingBuffer;
use crate::time_source::TimeSource;

const ANALYSIS_WINDOW_SECS: usize = 5;
const ANALYSIS_CADENCE: Duration = Duration::from_secs(1);

pub fn run(
    ring_buffer: Arc<RingBuffer>,
    time_source: Arc<dyn TimeSource>,
    sample_rate: u32,
    mut detector: Box<dyn BeatDetector>,
    confirmed_beats: Arc<Mutex<Vec<ConfirmedBeat>>>,
) {
    let window_samples = sample_rate as usize * ANALYSIS_WINDOW_SECS;
    let mut confirmation = BeatConfirmation::new();
    let mut run_id: usize = 0;

    #[cfg(debug_assertions)]
    let mut compute_times: Vec<f64> = Vec::new();

    loop {
        let tick_start = time_source.now();
        let anchor_time = tick_start;

        let samples = ring_buffer.read_latest(window_samples);
        if samples.len() < window_samples {
            time_source.sleep_until(tick_start + ANALYSIS_CADENCE);
            continue;
        }

        let beat_positions = detector.detect(&samples, sample_rate);

        let compute_time = tick_start.elapsed();

        #[cfg(debug_assertions)]
        {
            let ct_secs = compute_time.as_secs_f64();
            compute_times.push(ct_secs);
            if compute_times.len() > 10 {
                compute_times.remove(0);
            }
            let avg: f64 = compute_times.iter().sum::<f64>() / compute_times.len() as f64;
            eprintln!(
                "[analysis] run {run_id}: {n_beats} candidates, compute {ct_secs:.3}s (avg {avg:.3}s)",
                n_beats = beat_positions.len()
            );
            let buffer_secs = ring_buffer.capacity() as f64 / sample_rate as f64;
            if avg + ANALYSIS_WINDOW_SECS as f64 > buffer_secs - 1.0 {
                eprintln!(
                    "[analysis] WARNING: headroom low — avg compute {avg:.1}s + {ANALYSIS_WINDOW_SECS}s window approaches {buffer_secs}s buffer"
                );
            }
        }

        let candidate_instants: Vec<std::time::Instant> = beat_positions
            .iter()
            .filter_map(|&pos| {
                let offset_from_end = ANALYSIS_WINDOW_SECS as f64 - pos as f64;
                if offset_from_end >= 0.0 {
                    anchor_time.checked_sub(Duration::from_secs_f64(offset_from_end))
                } else {
                    Some(anchor_time + Duration::from_secs_f64(-offset_from_end))
                }
            })
            .collect();

        confirmation.add_candidates(candidate_instants);
        let newly_confirmed = confirmation.confirm();

        if !newly_confirmed.is_empty()
            && let Ok(mut beats) = confirmed_beats.lock()
        {
            *beats = newly_confirmed;
        }

        run_id += 1;

        let elapsed = tick_start.elapsed();
        if elapsed < ANALYSIS_CADENCE {
            time_source.sleep_until(tick_start + ANALYSIS_CADENCE);
        }
    }
}
