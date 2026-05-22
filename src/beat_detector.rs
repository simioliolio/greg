use std::path::Path;

pub trait BeatDetector: Send {
    fn detect(&mut self, samples: &[f32], sample_rate: u32) -> Vec<f32>;
}

pub struct BeatThisDetector {
    beat_this: beat_this::BeatThis<<beat_this::RtenRuntime as beat_this::Runtime>::Model>,
}

impl BeatThisDetector {
    pub fn new(mel_model_path: &Path, beat_model_path: &Path) -> anyhow::Result<Self> {
        let runtime = beat_this::RtenRuntime;
        let beat_this = beat_this::BeatThis::new(&runtime, mel_model_path, beat_model_path)?;
        Ok(Self { beat_this })
    }
}

impl BeatDetector for BeatThisDetector {
    fn detect(&mut self, samples: &[f32], sample_rate: u32) -> Vec<f32> {
        match self.beat_this.analyze_audio_timed(samples, sample_rate) {
            Ok(timed) => timed.analysis.beats,
            Err(e) => {
                eprintln!("beat-this error: {e}");
                Vec::new()
            }
        }
    }
}

pub struct StubDetector {
    bpm: f64,
}

impl StubDetector {
    pub fn new(bpm: f64) -> Self {
        Self { bpm }
    }
}

impl BeatDetector for StubDetector {
    fn detect(&mut self, samples: &[f32], sample_rate: u32) -> Vec<f32> {
        let duration = samples.len() as f64 / sample_rate as f64;
        let interval = 60.0 / self.bpm;
        let mut beats = Vec::new();
        let mut t = interval;
        while t < duration {
            beats.push(t as f32);
            t += interval;
        }
        beats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_detector_120bpm() {
        let mut detector = StubDetector::new(120.0);
        let sample_rate = 44100;
        let duration_secs = 5.0;
        let samples = vec![0.0f32; (sample_rate as f64 * duration_secs) as usize];

        let beats = detector.detect(&samples, sample_rate);

        let interval = 0.5;
        assert_eq!(beats.len(), 9);
        for (i, &beat) in beats.iter().enumerate() {
            let expected = interval * (i + 1) as f64;
            assert!(
                (beat as f64 - expected).abs() < 0.001,
                "beat {i}: expected {expected}, got {beat}"
            );
        }
    }

    #[test]
    fn stub_detector_empty_buffer() {
        let mut detector = StubDetector::new(120.0);
        let beats = detector.detect(&[], 44100);
        assert!(beats.is_empty());
    }

    #[test]
    fn beat_this_detector_invalid_path() {
        let result = BeatThisDetector::new(
            Path::new("/nonexistent/mel.onnx"),
            Path::new("/nonexistent/beat.onnx"),
        );
        assert!(result.is_err());
    }
}
