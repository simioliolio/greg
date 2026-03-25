use aubio_rs::{OnsetMode, Tempo};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::bounded;

fn main() {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .expect("no input device available");

    println!("Using input device: {}", device.name().unwrap());

    let config = device.default_input_config().unwrap();
    println!(
        "Default input config: channels={}, sample_rate={}, format={:?}",
        config.channels(),
        config.sample_rate().0,
        config.sample_format()
    );

    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;

    let buf_size = 1024;
    let hop_size = 512;

    // Bounded channel: audio callback sends mono samples, processing thread receives
    let (tx, rx) = bounded::<Vec<f32>>(64);

    // Processing thread — owns the Tempo detector, no sharing needed
    let handle = std::thread::spawn(move || {
        let mut tempo = Tempo::new(OnsetMode::default(), buf_size, hop_size, sample_rate)
            .expect("failed to create tempo detector");
        let mut buf = Vec::with_capacity(hop_size);

        while let Ok(samples) = rx.recv() {
            buf.extend_from_slice(&samples);

            while buf.len() >= hop_size {
                let hop_data: Vec<f32> = buf.drain(..hop_size).collect();
                let beat = tempo.do_result(&hop_data).unwrap();

                if beat > 0.0 {
                    let bpm = tempo.get_bpm();
                    let confidence = tempo.get_confidence();
                    println!("BEAT  |  bpm: {bpm:.1}  confidence: {confidence:.3}");
                }
            }
        }
    });

    let stream = device
        .build_input_stream(
            &config.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // Mix down to mono
                let mono: Vec<f32> = data
                    .chunks(channels)
                    .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                    .collect();

                // Non-blocking: drop samples if the processing thread falls behind
                let _ = tx.try_send(mono);
            },
            |err| eprintln!("stream error: {err}"),
            None,
        )
        .expect("failed to build input stream");

    stream.play().expect("failed to start stream");

    println!("Listening for beats... Press Ctrl+C to stop.");
    handle.join().unwrap();
}
