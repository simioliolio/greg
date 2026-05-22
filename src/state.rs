use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemState {
    Idle = 0,
    RateLocking = 1,
    Locked = 2,
}

impl SystemState {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Idle,
            1 => Self::RateLocking,
            2 => Self::Locked,
            _ => Self::Idle,
        }
    }

    pub fn midi_active(self) -> bool {
        self != Self::Idle
    }
}

pub struct SharedState {
    state: AtomicU8,
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(SystemState::Idle as u8),
        }
    }

    pub fn get(&self) -> SystemState {
        SystemState::from_u8(self.state.load(Ordering::Acquire))
    }

    pub fn set(&self, new_state: SystemState) {
        let old = self.get();
        if old != new_state {
            eprintln!("[state] {old:?} → {new_state:?}");
        }
        self.state.store(new_state as u8, Ordering::Release);
    }

    pub fn midi_active(&self) -> bool {
        self.get().midi_active()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_idle() {
        let state = SharedState::new();
        assert_eq!(state.get(), SystemState::Idle);
        assert!(!state.midi_active());
    }

    #[test]
    fn transitions() {
        let state = SharedState::new();

        state.set(SystemState::RateLocking);
        assert_eq!(state.get(), SystemState::RateLocking);
        assert!(state.midi_active());

        state.set(SystemState::Locked);
        assert_eq!(state.get(), SystemState::Locked);
        assert!(state.midi_active());

        state.set(SystemState::Idle);
        assert_eq!(state.get(), SystemState::Idle);
        assert!(!state.midi_active());
    }

    #[test]
    fn midi_active_only_when_not_idle() {
        assert!(!SystemState::Idle.midi_active());
        assert!(SystemState::RateLocking.midi_active());
        assert!(SystemState::Locked.midi_active());
    }
}
