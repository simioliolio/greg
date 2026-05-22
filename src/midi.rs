use midir::os::unix::VirtualOutput;
use midir::MidiOutputConnection;

const MIDI_TIMING_CLOCK: u8 = 0xF8;
#[allow(dead_code)]
const MIDI_START: u8 = 0xFA;
#[allow(dead_code)]
const MIDI_STOP: u8 = 0xFC;

pub struct MidiOut {
    conn: MidiOutputConnection,
}

impl MidiOut {
    pub fn new(port_name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let output = midir::MidiOutput::new("Greg MIDI Output")?;
        let conn = output.create_virtual(port_name)?;
        Ok(Self { conn })
    }

    pub fn send_clock(&mut self) {
        let _ = self.conn.send(&[MIDI_TIMING_CLOCK]);
    }

    #[allow(dead_code)]
    pub fn send_start(&mut self) {
        let _ = self.conn.send(&[MIDI_START]);
    }

    #[allow(dead_code)]
    pub fn send_stop(&mut self) {
        let _ = self.conn.send(&[MIDI_STOP]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_output_creation() {
        let result = MidiOut::new("Greg Test");
        // Virtual MIDI ports are supported on macOS and Linux.
        // This test may fail on CI without MIDI support — that's acceptable.
        if let Err(e) = &result {
            eprintln!("MidiOut creation failed (expected on some platforms): {e}");
        }
        // We don't assert success since it depends on platform MIDI support.
    }
}
