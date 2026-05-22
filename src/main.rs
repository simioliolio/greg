mod analysis;
pub mod audio_source;
mod beat_confirmation;
pub mod beat_detector;
pub mod click_track;
mod clock;
mod input;
mod midi;
mod pll;
pub mod ring_buffer;
mod state;
pub mod system;
mod tap_tempo;
pub mod time_source;

use std::sync::Arc;
use std::time::Duration;

use audio_source::{AudioSource, CpalSource};
use time_source::{RealTimeSource, TimeSource};

fn main() {
    let source = CpalSource::new();
    let sample_rate = source.sample_rate();
    let time_source: Arc<dyn TimeSource> = Arc::new(RealTimeSource);

    let config = system::SystemConfig {
        sample_rate,
        buffer_duration_secs: 12,
        initial_bpm: 120.0,
    };

    let sys = system::System::new(config, Arc::clone(&time_source));
    let running = sys.run(Box::new(source), true);

    println!(
        "Capturing audio into {}s ring buffer ({} samples).",
        config.buffer_duration_secs,
        running.ring_buffer().capacity()
    );
    println!("Tap SPACE 3 times to set tempo. Press Ctrl+C to stop.");

    loop {
        let next = time_source.now() + Duration::from_millis(500);
        let pos = running.ring_buffer().write_position();
        let secs = pos as f64 / sample_rate as f64;

        let recent = running.ring_buffer().read_latest(sample_rate as usize);
        let peak = recent.iter().fold(0.0_f32, |a, &b| a.max(b.abs()));

        let beat_info = running.confirmed_beats().lock().map(|beats| {
            let count = beats.len();
            let latest = beats.last().map(|b| {
                let age = b.timestamp.elapsed().as_secs_f64();
                format!("{age:.1}s ago")
            });
            (count, latest)
        });
        let (beat_count, latest) = beat_info.unwrap_or((0, None));
        let latest_str = latest.as_deref().unwrap_or("-");

        let bpm = running.clock_state().effective_bpm();
        let current_state = running.system_state().get();

        println!(
            "cursor: {pos} ({secs:.1}s)  peak: {peak:.4}  confirmed: {beat_count} (latest: {latest_str})  clock: {bpm:.1}BPM  state: {current_state:?}"
        );
        time_source.sleep_until(next);
    }
}
