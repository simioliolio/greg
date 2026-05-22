use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::analysis;
use crate::audio_source::AudioSource;
use crate::beat_confirmation::ConfirmedBeat;
use crate::beat_detector;
use crate::clock::{self, ClockState};
use crate::input;
use crate::midi;
use crate::ring_buffer::RingBuffer;
use crate::state::SharedState;
use crate::time_source::TimeSource;

#[derive(Clone, Copy)]
pub struct SystemConfig {
    pub sample_rate: u32,
    pub buffer_duration_secs: usize,
    pub initial_bpm: f64,
}

pub struct System {
    config: SystemConfig,
    time_source: Arc<dyn TimeSource>,
    ring_buffer: Arc<RingBuffer>,
    confirmed_beats: Arc<Mutex<Vec<ConfirmedBeat>>>,
    system_state: Arc<SharedState>,
    clock_state: Arc<ClockState>,
}

impl System {
    pub fn new(config: SystemConfig, time_source: Arc<dyn TimeSource>) -> Self {
        let ring_buffer = Arc::new(RingBuffer::new(
            config.sample_rate as usize * config.buffer_duration_secs,
        ));
        let confirmed_beats = Arc::new(Mutex::new(Vec::new()));
        let system_state = Arc::new(SharedState::new());
        let clock_state = Arc::new(ClockState::new(config.initial_bpm));

        Self {
            config,
            time_source,
            ring_buffer,
            confirmed_beats,
            system_state,
            clock_state,
        }
    }

    pub fn run(self, audio_source: Box<dyn AudioSource>, enable_input: bool) -> RunningSystem {
        let detector = make_detector();

        let rb_analysis = Arc::clone(&self.ring_buffer);
        let ts_analysis = Arc::clone(&self.time_source);
        let cb_analysis = Arc::clone(&self.confirmed_beats);
        let sample_rate = self.config.sample_rate;

        std::thread::spawn(move || {
            analysis::run(rb_analysis, ts_analysis, sample_rate, detector, cb_analysis);
        });

        let (tap_tx, tap_rx) = crossbeam_channel::unbounded();

        let ts_clock = Arc::clone(&self.time_source);
        let cs_clock = Arc::clone(&self.clock_state);
        let ss_clock = Arc::clone(&self.system_state);
        let cb_clock = Arc::clone(&self.confirmed_beats);

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
            clock::run(cs_clock, ss_clock, ts_clock, tap_rx, cb_clock, |gate_open| {
                if gate_open
                    && let Some(ref mut m) = midi_out
                {
                    m.send_clock();
                }
            });
        });

        if enable_input {
            let ts_input = Arc::clone(&self.time_source);
            std::thread::spawn(move || {
                input::run(ts_input, tap_tx);
            });
        }

        let rb_audio = Arc::clone(&self.ring_buffer);
        std::thread::spawn(move || {
            audio_source.run(rb_audio);
        });

        RunningSystem {
            ring_buffer: self.ring_buffer,
            confirmed_beats: self.confirmed_beats,
            system_state: self.system_state,
            clock_state: self.clock_state,
        }
    }

    pub fn run_headless(
        self,
        audio_source: Box<dyn AudioSource>,
        detector: Box<dyn beat_detector::BeatDetector>,
        on_pulse: impl FnMut(bool) + Send + 'static,
    ) -> RunningSystem {
        let rb_analysis = Arc::clone(&self.ring_buffer);
        let ts_analysis = Arc::clone(&self.time_source);
        let cb_analysis = Arc::clone(&self.confirmed_beats);
        let sample_rate = self.config.sample_rate;

        std::thread::spawn(move || {
            analysis::run(rb_analysis, ts_analysis, sample_rate, detector, cb_analysis);
        });

        let (_tap_tx, tap_rx) = crossbeam_channel::unbounded();

        let ts_clock = Arc::clone(&self.time_source);
        let cs_clock = Arc::clone(&self.clock_state);
        let ss_clock = Arc::clone(&self.system_state);
        let cb_clock = Arc::clone(&self.confirmed_beats);

        std::thread::spawn(move || {
            clock::run(cs_clock, ss_clock, ts_clock, tap_rx, cb_clock, on_pulse);
        });

        let rb_audio = Arc::clone(&self.ring_buffer);
        std::thread::spawn(move || {
            audio_source.run(rb_audio);
        });

        RunningSystem {
            ring_buffer: self.ring_buffer,
            confirmed_beats: self.confirmed_beats,
            system_state: self.system_state,
            clock_state: self.clock_state,
        }
    }
}

pub struct RunningSystem {
    ring_buffer: Arc<RingBuffer>,
    confirmed_beats: Arc<Mutex<Vec<ConfirmedBeat>>>,
    system_state: Arc<SharedState>,
    clock_state: Arc<ClockState>,
}

impl RunningSystem {
    pub fn ring_buffer(&self) -> &RingBuffer {
        &self.ring_buffer
    }

    pub fn confirmed_beats(&self) -> &Mutex<Vec<ConfirmedBeat>> {
        &self.confirmed_beats
    }

    pub fn system_state(&self) -> &SharedState {
        &self.system_state
    }

    pub fn clock_state(&self) -> &ClockState {
        &self.clock_state
    }
}

fn make_detector() -> Box<dyn beat_detector::BeatDetector> {
    let mel_model = PathBuf::from("models/mel_spectrogram.onnx");
    let beat_model = PathBuf::from("models/beat_this.onnx");

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
    }
}
