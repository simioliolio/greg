use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::ring_buffer::RingBuffer;
use crate::time_source::SyntheticTimeSource;

const WAV_CHUNK_SIZE: usize = 1024;

pub trait AudioSource: Send {
    fn sample_rate(&self) -> u32;
    fn run(self: Box<Self>, ring_buffer: Arc<RingBuffer>);
}

pub struct CpalSource {
    device: cpal::Device,
    config: cpal::SupportedStreamConfig,
}

impl Default for CpalSource {
    fn default() -> Self {
        Self::new()
    }
}

impl CpalSource {
    pub fn new() -> Self {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .expect("no input device available");

        println!("Using input device: {}", device.name().unwrap());

        let config = device.default_input_config().unwrap();
        let channels = config.channels();
        let sample_rate = config.sample_rate().0;

        println!(
            "Input config: channels={channels}, sample_rate={sample_rate}, format={:?}",
            config.sample_format()
        );

        Self { device, config }
    }
}

impl AudioSource for CpalSource {
    fn sample_rate(&self) -> u32 {
        self.config.sample_rate().0
    }

    fn run(self: Box<Self>, ring_buffer: Arc<RingBuffer>) {
        let channels = self.config.channels() as usize;

        let stream = self
            .device
            .build_input_stream(
                &self.config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mono: Vec<f32> = data
                        .chunks(channels)
                        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                        .collect();
                    ring_buffer.write(&mono);
                },
                |err| eprintln!("stream error: {err}"),
                None,
            )
            .expect("failed to build input stream");

        stream.play().expect("failed to start stream");

        loop {
            std::thread::park();
        }
    }
}

pub struct WavSource {
    samples: Vec<f32>,
    file_sample_rate: u32,
    target_sample_rate: u32,
    synthetic_time: Arc<SyntheticTimeSource>,
}

impl WavSource {
    pub fn from_file(
        path: &Path,
        target_sample_rate: u32,
        synthetic_time: Arc<SyntheticTimeSource>,
    ) -> Self {
        let mut reader = hound::WavReader::open(path).expect("failed to open WAV file");
        let spec = reader.spec();

        let raw_samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
            hound::SampleFormat::Int => {
                let max = (1 << (spec.bits_per_sample - 1)) as f32;
                reader
                    .samples::<i32>()
                    .map(|s| s.unwrap() as f32 / max)
                    .collect()
            }
        };

        let mono: Vec<f32> = if spec.channels > 1 {
            raw_samples
                .chunks(spec.channels as usize)
                .map(|frame| frame.iter().sum::<f32>() / spec.channels as f32)
                .collect()
        } else {
            raw_samples
        };

        Self {
            samples: mono,
            file_sample_rate: spec.sample_rate,
            target_sample_rate,
            synthetic_time,
        }
    }

    pub fn from_samples(
        samples: Vec<f32>,
        sample_rate: u32,
        synthetic_time: Arc<SyntheticTimeSource>,
    ) -> Self {
        Self {
            samples,
            file_sample_rate: sample_rate,
            target_sample_rate: sample_rate,
            synthetic_time,
        }
    }
}

impl AudioSource for WavSource {
    fn sample_rate(&self) -> u32 {
        self.target_sample_rate
    }

    fn run(self: Box<Self>, ring_buffer: Arc<RingBuffer>) {
        let samples = if self.file_sample_rate != self.target_sample_rate {
            resample(&self.samples, self.file_sample_rate, self.target_sample_rate)
        } else {
            self.samples
        };

        for chunk in samples.chunks(WAV_CHUNK_SIZE) {
            ring_buffer.write(chunk);
            let chunk_duration =
                Duration::from_secs_f64(chunk.len() as f64 / self.target_sample_rate as f64);
            self.synthetic_time.advance_by(chunk_duration);
        }
    }
}

fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    let ratio = to_rate as f64 / from_rate as f64;
    let new_len = (samples.len() as f64 * ratio) as usize;
    let mut result = Vec::with_capacity(new_len);
    for i in 0..new_len {
        let src_pos = i as f64 / ratio;
        let idx = src_pos as usize;
        let frac = src_pos - idx as f64;
        let sample = if idx + 1 < samples.len() {
            samples[idx] * (1.0 - frac as f32) + samples[idx + 1] * frac as f32
        } else if idx < samples.len() {
            samples[idx]
        } else {
            0.0
        };
        result.push(sample);
    }
    result
}
