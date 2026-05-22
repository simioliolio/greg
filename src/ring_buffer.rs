use std::sync::atomic::{AtomicUsize, Ordering};

pub struct RingBuffer {
    buffer: Box<[f32]>,
    capacity: usize,
    write_pos: AtomicUsize,
}

// SAFETY: The ring buffer is designed for single-writer, single-reader concurrent access.
// The writer (audio callback) writes samples and publishes the position via Release ordering.
// The reader (analysis thread) loads the position via Acquire ordering and only reads data
// behind the published cursor. The 12s buffer size guarantees the writer never laps the reader
// (5s analysis window + compute time < 7s remaining margin). See ADR-0002.
unsafe impl Sync for RingBuffer {}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0.0; capacity].into_boxed_slice(),
            capacity,
            write_pos: AtomicUsize::new(0),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn write_position(&self) -> usize {
        self.write_pos.load(Ordering::Acquire)
    }

    /// Write samples into the ring buffer. Lock-free — safe to call from the audio callback.
    pub fn write(&self, samples: &[f32]) {
        let pos = self.write_pos.load(Ordering::Relaxed);
        let buf_ptr = self.buffer.as_ptr() as *mut f32;

        for (i, &sample) in samples.iter().enumerate() {
            let idx = (pos + i) % self.capacity;
            // SAFETY: idx is always in bounds (modulo capacity), and only one thread writes.
            unsafe {
                *buf_ptr.add(idx) = sample;
            }
        }

        self.write_pos
            .store(pos + samples.len(), Ordering::Release);
    }

    /// Read the most recent `count` samples from the buffer.
    /// Returns fewer samples if not enough data has been written yet.
    pub fn read_latest(&self, count: usize) -> Vec<f32> {
        let pos = self.write_pos.load(Ordering::Acquire);

        let available = pos.min(count).min(self.capacity);
        if available == 0 {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(available);
        let start = pos - available;

        for i in 0..available {
            let idx = (start + i) % self.capacity;
            // SAFETY: idx is in bounds. The writer is ahead of this region — we only read
            // data that was written before the Release store we observed.
            let sample = unsafe { *self.buffer.as_ptr().add(idx) };
            result.push(sample);
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read_back() {
        let rb = RingBuffer::new(1024);
        let samples: Vec<f32> = (0..100).map(|i| i as f32).collect();
        rb.write(&samples);

        assert_eq!(rb.write_position(), 100);

        let read = rb.read_latest(100);
        assert_eq!(read, samples);
    }

    #[test]
    fn read_latest_fewer_than_written() {
        let rb = RingBuffer::new(1024);
        let samples: Vec<f32> = (0..100).map(|i| i as f32).collect();
        rb.write(&samples);

        let read = rb.read_latest(10);
        let expected: Vec<f32> = (90..100).map(|i| i as f32).collect();
        assert_eq!(read, expected);
    }

    #[test]
    fn read_more_than_written_returns_available() {
        let rb = RingBuffer::new(1024);
        let samples: Vec<f32> = (0..50).map(|i| i as f32).collect();
        rb.write(&samples);

        let read = rb.read_latest(200);
        assert_eq!(read, samples);
    }

    #[test]
    fn wraparound() {
        let rb = RingBuffer::new(64);

        // Write more than capacity to trigger wrap
        let first: Vec<f32> = (0..60).map(|i| i as f32).collect();
        rb.write(&first);

        let second: Vec<f32> = (60..80).map(|i| i as f32).collect();
        rb.write(&second);

        assert_eq!(rb.write_position(), 80);

        // Read last 64 samples (full capacity)
        let read = rb.read_latest(64);
        let expected: Vec<f32> = (16..80).map(|i| i as f32).collect();
        assert_eq!(read, expected);
    }

    #[test]
    fn concurrent_write_and_read() {
        use std::sync::Arc;
        use std::thread;

        let rb = Arc::new(RingBuffer::new(48000 * 12));
        let rb_writer = Arc::clone(&rb);

        let writer = thread::spawn(move || {
            let chunk: Vec<f32> = (0..1024).map(|i| (i % 256) as f32).collect();
            for _ in 0..100 {
                rb_writer.write(&chunk);
            }
        });

        let rb_reader = Arc::clone(&rb);
        let reader = thread::spawn(move || {
            let mut reads = 0;
            while reads < 50 {
                let data = rb_reader.read_latest(4096);
                if !data.is_empty() {
                    // Verify all values are in expected range
                    for &sample in &data {
                        assert!(
                            (0.0..256.0).contains(&sample),
                            "unexpected sample value: {sample}"
                        );
                    }
                    reads += 1;
                }
                thread::yield_now();
            }
        });

        writer.join().unwrap();
        reader.join().unwrap();
    }

    #[test]
    fn empty_read() {
        let rb = RingBuffer::new(1024);
        let read = rb.read_latest(100);
        assert!(read.is_empty());
    }
}
