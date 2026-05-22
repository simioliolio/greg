mod ring_buffer;
mod time_source;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ring_buffer::RingBuffer;
use std::sync::Arc;
use std::time::Duration;
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

    let time_source = RealTimeSource;
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

        println!("cursor: {pos} samples ({secs:.1}s)  peak: {peak:.4}");
        time_source.sleep_until(next);
    }
}
