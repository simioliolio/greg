mod analysis;
mod beat_confirmation;
mod beat_detector;
mod clock;
mod midi;
mod ring_buffer;
mod time_source;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ring_buffer::RingBuffer;
use time_source::{RealTimeSource, TimeSource};

fn main() {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .expect("no input device available");

    println!("Using input device: {}", device.name().unwrap());

    let config = device.default_input_config().unwrap();
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;

    println!(
        "Input config: channels={channels}, sample_rate={sample_rate}, format={:?}",
        config.sample_format()
    );

    let buffer_duration_secs = 12;
    let ring_buffer = Arc::new(RingBuffer::new(sample_rate as usize * buffer_duration_secs));
    let rb_writer = Arc::clone(&ring_buffer);

    let stream = device
        .build_input_stream(
            &config.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let mono: Vec<f32> = data
                    .chunks(channels)
                    .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                    .collect();
                rb_writer.write(&mono);
            },
            |err| eprintln!("stream error: {err}"),
            None,
        )
        .expect("failed to build input stream");

    stream.play().expect("failed to start stream");

    let time_source: Arc<dyn TimeSource> = Arc::new(RealTimeSource);
    let confirmed_beats = Arc::new(Mutex::new(Vec::new()));

    // Beat detector setup
    let mel_model = PathBuf::from("models/mel_spectrogram.onnx");
    let beat_model = PathBuf::from("models/beat_this.onnx");

    let detector: Box<dyn beat_detector::BeatDetector> =
        if mel_model.exists() && beat_model.exists() {
            println!("Loading beat-this models...");
            match beat_detector::BeatThisDetector::new(&mel_model, &beat_model) {
                Ok(d) => {
                    println!("beat-this loaded successfully");
                    Box::new(d)
                }
                Err(e) => {
                    eprintln!("Failed to load beat-this: {e}. Using stub detector at 120 BPM.");
                    Box::new(beat_detector::StubDetector::new(120.0))
                }
            }
        } else {
            println!(
                "Models not found at models/. Using stub detector at 120 BPM. \
                 Run scripts/download-models.sh to get real models."
            );
            Box::new(beat_detector::StubDetector::new(120.0))
        };

    // Analysis thread
    let rb_analysis = Arc::clone(&ring_buffer);
    let ts_analysis = Arc::clone(&time_source);
    let cb_analysis = Arc::clone(&confirmed_beats);

    std::thread::spawn(move || {
        analysis::run(rb_analysis, ts_analysis, sample_rate, detector, cb_analysis);
    });

    // Clock thread + MIDI output
    let clock_state = Arc::new(clock::ClockState::new(120.0));
    let ts_clock = Arc::clone(&time_source);
    let clock_state_clone = Arc::clone(&clock_state);

    let mut midi_out = match midi::MidiOut::new("Greg") {
        Ok(m) => {
            println!("Virtual MIDI port 'Greg' created");
            Some(m)
        }
        Err(e) => {
            eprintln!("Failed to create MIDI port: {e}. Running without MIDI output.");
            None
        }
    };

    std::thread::spawn(move || {
        clock::run(clock_state_clone, ts_clock, |gate_open| {
            if gate_open
                && let Some(ref mut m) = midi_out
            {
                m.send_clock();
            }
        });
    });

    println!(
        "Capturing audio into {buffer_duration_secs}s ring buffer ({} samples). Press Ctrl+C to stop.",
        ring_buffer.capacity()
    );

    loop {
        let next = time_source.now() + Duration::from_millis(500);
        let pos = ring_buffer.write_position();
        let secs = pos as f64 / sample_rate as f64;

        let recent = ring_buffer.read_latest(sample_rate as usize);
        let peak = recent.iter().fold(0.0_f32, |a, &b| a.max(b.abs()));

        let beat_info = confirmed_beats.lock().map(|beats| {
            let count = beats.len();
            let latest = beats.last().map(|b| {
                let age = b.timestamp.elapsed().as_secs_f64();
                format!("{age:.1}s ago")
            });
            (count, latest)
        });
        let (beat_count, latest) = beat_info.unwrap_or((0, None));
        let latest_str = latest.as_deref().unwrap_or("-");

        let bpm = clock_state.effective_bpm();
        let gate = if clock_state.midi_gate.load(std::sync::atomic::Ordering::Relaxed) {
            "ON"
        } else {
            "OFF"
        };

        println!(
            "cursor: {pos} ({secs:.1}s)  peak: {peak:.4}  confirmed: {beat_count} (latest: {latest_str})  clock: {bpm:.1}BPM gate={gate}"
        );
        time_source.sleep_until(next);
    }
}
