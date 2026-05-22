use std::f32::consts::PI;
use std::path::Path;

const CLICK_DURATION_SECS: f64 = 0.005;
const CLICK_FREQUENCY_HZ: f64 = 1000.0;

pub fn generate_click_track(bpm: f64, duration_secs: f64, sample_rate: u32) -> Vec<f32> {
    let total_samples = (duration_secs * sample_rate as f64) as usize;
    let mut samples = vec![0.0f32; total_samples];

    let beat_interval_secs = 60.0 / bpm;
    let click_samples = (CLICK_DURATION_SECS * sample_rate as f64) as usize;

    let mut beat_time = beat_interval_secs;
    while beat_time < duration_secs {
        let start_sample = (beat_time * sample_rate as f64) as usize;
        for i in 0..click_samples {
            let idx = start_sample + i;
            if idx >= total_samples {
                break;
            }
            let t = i as f64 / sample_rate as f64;
            let envelope = 1.0 - (i as f64 / click_samples as f64);
            samples[idx] = (2.0 * PI as f64 * CLICK_FREQUENCY_HZ * t).sin() as f32
                * envelope as f32
                * 0.8;
        }
        beat_time += beat_interval_secs;
    }

    samples
}

pub fn write_click_track_wav(path: &Path, bpm: f64, duration_secs: f64, sample_rate: u32) {
    let samples = generate_click_track(bpm, duration_secs, sample_rate);
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("failed to create WAV file");
    for sample in &samples {
        writer.write_sample(*sample).unwrap();
    }
    writer.finalize().unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_track_120bpm_has_beats() {
        let samples = generate_click_track(120.0, 5.0, 44100);
        assert_eq!(samples.len(), 44100 * 5);

        let beat_interval_samples = (44100.0 * 60.0 / 120.0) as usize;
        let mut found_clicks = 0;
        for beat_idx in 1..10 {
            let pos = beat_idx * beat_interval_samples;
            if pos < samples.len() {
                let end = (pos + 220).min(samples.len());
                let region = &samples[pos..end];
                let peak = region.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
                if peak > 0.1 {
                    found_clicks += 1;
                }
            }
        }
        assert!(found_clicks >= 5, "expected clicks, found {found_clicks}");
    }

    #[test]
    fn click_track_wav_roundtrip() {
        let dir = std::env::temp_dir().join("greg_test_click");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("test_120bpm.wav");

        write_click_track_wav(&path, 120.0, 2.0, 44100);
        assert!(path.exists());

        let reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, 44100);
        assert_eq!(spec.channels, 1);

        std::fs::remove_file(&path).ok();
        std::fs::remove_dir(&dir).ok();
    }
}
