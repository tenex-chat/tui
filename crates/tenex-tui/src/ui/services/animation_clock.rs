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

    /// Get the wave offset for character-by-character color animation
    /// Returns a value that changes over time, suitable for creating a wave effect
    pub fn wave_offset(&self) -> usize {
        // Divide by 2 to slow down the animation (every 2 frames = ~200ms at 10fps)
        (self.frame_counter / 2) as usize
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
    fn test_wave_offset() {
        let mut clock = AnimationClock::new();
        let offset1 = clock.wave_offset();
        assert_eq!(offset1, 0); // Frame 0
        clock.tick(); // Frame 1
        assert_eq!(clock.wave_offset(), 0); // Still 0 (divided by 2)
        clock.tick(); // Frame 2
        assert_eq!(clock.wave_offset(), 1); // Now 1
        clock.tick(); // Frame 3
        assert_eq!(clock.wave_offset(), 1); // Still 1
        clock.tick(); // Frame 4
        assert_eq!(clock.wave_offset(), 2); // Now 2
        clock.tick(); // Frame 5
        assert_eq!(clock.wave_offset(), 2); // Still 2
        clock.tick(); // Frame 6
        assert_eq!(clock.wave_offset(), 3); // Now 3
    }

    #[test]
    fn test_wave_offset_wrap_behavior() {
        let mut clock = AnimationClock::new();

        // Test wrapping at u64::MAX
        // Set frame_counter to near maximum to test wrap behavior
        clock.frame_counter = u64::MAX;
        let offset_before = clock.wave_offset();

        // Tick should wrap from u64::MAX to 0 using wrapping_add
        clock.tick();
        assert_eq!(clock.frame_counter, 0); // Wrapped to 0
        assert_eq!(clock.wave_offset(), 0); // Wave offset should also be 0

        // Verify offset was correct before wrap (u64::MAX / 2 rounds down)
        assert_eq!(offset_before, (u64::MAX / 2) as usize);
    }

    #[test]
    fn test_wave_offset_long_run_stability() {
        let mut clock = AnimationClock::new();

        // Simulate a long-running session (10,000 frames)
        // At 10fps, this is ~16.6 minutes of runtime
        for _ in 0..10_000 {
            clock.tick();
        }

        // Verify wave_offset is still producing reasonable values
        let offset = clock.wave_offset();
        assert_eq!(offset, 5_000); // 10,000 / 2 = 5,000

        // Verify it continues to increment properly
        clock.tick();
        clock.tick();
        assert_eq!(clock.wave_offset(), 5_001);

        // Test even longer run (simulate 24 hours at 10fps = 864,000 frames)
        clock.frame_counter = 864_000;
        assert_eq!(clock.wave_offset(), 432_000);

        // Verify no precision loss or unexpected behavior
        clock.tick();
        assert_eq!(clock.wave_offset(), 432_000); // Still same (864,001 / 2 = 432,000)
        clock.tick();
        assert_eq!(clock.wave_offset(), 432_001); // Now increments (864,002 / 2 = 432,001)
    }
}
