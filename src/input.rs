use std::sync::Arc;

use crossbeam_channel::Sender;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};

use crate::tap_tempo::{TapTempo, TapResult};
use crate::time_source::TimeSource;

pub struct TapEvent {
    pub bpm: f64,
}

pub fn run(
    time_source: Arc<dyn TimeSource>,
    tap_tx: Sender<TapEvent>,
) {
    crossterm::terminal::enable_raw_mode().expect("failed to enable raw mode");

    let mut tap_tempo = TapTempo::new();

    loop {
        match event::read() {
            Ok(Event::Key(key_event)) => {
                if key_event.kind != KeyEventKind::Press {
                    continue;
                }
                match key_event.code {
                    KeyCode::Char(' ') => {
                        let now = time_source.now();
                        if let Some(TapResult { bpm }) = tap_tempo.tap(now) {
                            let _ = tap_tx.send(TapEvent { bpm });
                        }
                    }
                    KeyCode::Char('c')
                        if key_event
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        crossterm::terminal::disable_raw_mode().ok();
                        std::process::exit(0);
                    }
                    _ => {}
                }
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("input error: {e}");
                break;
            }
        }
    }

    crossterm::terminal::disable_raw_mode().ok();
}
