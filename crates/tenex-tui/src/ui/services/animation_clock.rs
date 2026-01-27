/// Manages UI animation timing for spinners and pulsing indicators.
/// This is a general-purpose animation clock used across the UI.
pub struct AnimationClock {
    /// Frame counter that advances each tick (~100ms)
    frame_counter: u64,
}

impl AnimationClock {
    pub fn new() -> Self {
        Self { frame_counter: 0 }
    }

    /// Advance the animation clock by one frame
    pub fn tick(&mut self) {
        self.frame_counter = self.frame_counter.wrapping_add(1);
    }

    /// Get current spinner character for loading animations
    pub fn spinner_char(&self) -> char {
        const SPINNERS: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        // Divide by 2 to slow down the animation (every 2 frames = ~200ms at 10fps)
        SPINNERS[(self.frame_counter / 2) as usize % SPINNERS.len()]
    }

    /// Get a pulsing indicator for activity displays (alternates every ~200ms)
    /// Returns true for "on" phase, false for "off" phase
    pub fn activity_pulse(&self) -> bool {
        // Pulse every 4 frames at 10fps = ~400ms cycle
        self.frame_counter % 4 < 2
    }

    /// Get the filled/empty activity indicator characters
    pub fn activity_indicator(&self) -> &'static str {
        if self.activity_pulse() {
            "◉"
        } else {
            "○"
        }
    }
}

impl Default for AnimationClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_animation_clock_new() {
        let clock = AnimationClock::new();
        // Just verify it constructs without panic
        let _ = clock.spinner_char();
    }

    #[test]
    fn test_animation_clock_tick() {
        let mut clock = AnimationClock::new();
        let initial_spinner = clock.spinner_char();
        // After 2 ticks, should advance to next spinner char
        clock.tick();
        clock.tick();
        let next_spinner = clock.spinner_char();
        assert_ne!(initial_spinner, next_spinner);
    }

    #[test]
    fn test_activity_pulse() {
        let mut clock = AnimationClock::new();
        // Frame 0: pulse is true (on)
        assert!(clock.activity_pulse());
        clock.tick(); // Frame 1: still on
        assert!(clock.activity_pulse());
        clock.tick(); // Frame 2: now off
        assert!(!clock.activity_pulse());
        clock.tick(); // Frame 3: still off
        assert!(!clock.activity_pulse());
        clock.tick(); // Frame 4: back on
        assert!(clock.activity_pulse());
    }

    #[test]
    fn test_activity_indicator() {
        let mut clock = AnimationClock::new();
        assert_eq!(clock.activity_indicator(), "◉"); // On at frame 0
        clock.tick();
        assert_eq!(clock.activity_indicator(), "◉"); // Still on at frame 1
        clock.tick();
        assert_eq!(clock.activity_indicator(), "○"); // Off at frame 2
        clock.tick();
        assert_eq!(clock.activity_indicator(), "○"); // Still off at frame 3
        clock.tick();
        assert_eq!(clock.activity_indicator(), "◉"); // Back on at frame 4
    }
}
