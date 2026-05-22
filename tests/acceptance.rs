use std::sync::Arc;
use std::time::{Duration, Instant};

use greg::audio_source::WavSource;
use greg::beat_detector::StubDetector;
use greg::click_track::generate_click_track;
use greg::system::{System, SystemConfig};
use greg::time_source::{SyntheticTimeSource, TimeSource};

const SAMPLE_RATE: u32 = 44100;

fn run_fast_forward(bpm: f64, audio_duration_secs: f64, settle_secs: f64) -> ConvergenceResult {
    let base = Instant::now();
    let time_source = Arc::new(SyntheticTimeSource::new(base));

    let config = SystemConfig {
        sample_rate: SAMPLE_RATE,
        buffer_duration_secs: 12,
        initial_bpm: 120.0,
    };

    let sys = System::new(config, Arc::clone(&time_source) as Arc<dyn greg::time_source::TimeSource>);

    let samples = generate_click_track(bpm, audio_duration_secs, SAMPLE_RATE);
    let source = WavSource::from_samples(samples, SAMPLE_RATE, Arc::clone(&time_source));
    let detector: Box<dyn greg::beat_detector::BeatDetector> = Box::new(StubDetector::new(bpm));

    let pulse_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let pc_clone = Arc::clone(&pulse_count);

    let running = sys.run_headless(Box::new(source), detector, move |_gate| {
        pc_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });

    // WavSource runs in its own thread and advances synthetic time as it delivers audio.
    // Wait for it to finish delivering all audio by checking if time has advanced
    // past the audio duration.
    let audio_end = base + Duration::from_secs_f64(audio_duration_secs);
    let deadline = Instant::now() + Duration::from_secs(30);
    while time_source.now() < audio_end {
        if Instant::now() > deadline {
            panic!("Timed out waiting for WavSource to deliver audio");
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    // Advance time further to let analysis pipeline settle
    let settle_steps = (settle_secs * 10.0) as usize;
    for _ in 0..settle_steps {
        time_source.advance_by(Duration::from_millis(100));
        std::thread::sleep(Duration::from_millis(1));
    }

    let final_state = running.system_state().get();
    let final_bpm = running.clock_state().effective_bpm();
    let confirmed = running
        .confirmed_beats()
        .lock()
        .map(|b| b.len())
        .unwrap_or(0);
    let pulses = pulse_count.load(std::sync::atomic::Ordering::Relaxed);

    ConvergenceResult {
        final_state,
        final_bpm,
        confirmed_beat_count: confirmed,
        pulse_count: pulses,
    }
}

struct ConvergenceResult {
    final_state: greg::state::SystemState,
    final_bpm: f64,
    confirmed_beat_count: usize,
    pulse_count: u64,
}

#[test]
fn convergence_120bpm() {
    let result = run_fast_forward(120.0, 20.0, 5.0);

    assert!(
        result.confirmed_beat_count >= 5,
        "expected at least 5 confirmed beats, got {}",
        result.confirmed_beat_count
    );

    assert!(
        result.pulse_count > 0,
        "expected clock pulses, got 0"
    );

    assert!(
        (result.final_bpm - 120.0).abs() < 10.0,
        "expected BPM near 120, got {:.1}",
        result.final_bpm
    );

    assert_ne!(
        result.final_state,
        greg::state::SystemState::Idle,
        "expected system to leave Idle state"
    );
}

// StubDetector only produces confirmable beats at BPMs where the beat interval
// divides evenly into the 1-second analysis cadence (e.g., 60, 120, 240 BPM).
// Non-aligned BPMs (like 140) require the real beat-this model for confirmation.

#[test]
fn convergence_60bpm() {
    let result = run_fast_forward(60.0, 60.0, 30.0);

    assert!(
        result.confirmed_beat_count >= 3,
        "expected at least 3 confirmed beats, got {}",
        result.confirmed_beat_count
    );

    assert!(
        result.pulse_count > 0,
        "expected clock pulses, got 0"
    );

    // Rate correction nudges 1 BPM per clock beat from initial 120 toward 60.
    // With 90s of synthetic time, BPM should have moved significantly toward 60.
    assert!(
        result.final_bpm < 120.0,
        "expected BPM to decrease from 120 toward 60, got {:.1}",
        result.final_bpm
    );

    assert_ne!(
        result.final_state,
        greg::state::SystemState::Idle,
        "expected system to leave Idle state"
    );
}
