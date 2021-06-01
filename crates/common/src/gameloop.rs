pub use self::timer::{TimeDelta, Timer};

mod timer {
    use std::time;

    #[derive(Debug)]
    pub struct TimeDelta(time::Duration);

    #[derive(Debug)]
    pub struct Timer {
        target_ticks: u16,
        target_delta: time::Duration,
        last_tick: instant::Instant,
        accumulated_delta: time::Duration,
        has_ticked: bool,
    }

    impl Timer {
        pub fn new(ticks_per_second: u16) -> Timer {
            let (target_seconds, target_nanos) = match ticks_per_second {
                0 => (std::u64::MAX, 0),
                1 => (1, 0),
                _ => (0, ((1.0 / f64::from(ticks_per_second)) * 1e9) as u32),
            };

            Timer {
                target_ticks: ticks_per_second,
                target_delta: time::Duration::new(target_seconds, target_nanos),
                last_tick: instant::Instant::now(),
                accumulated_delta: time::Duration::from_secs(0),
                has_ticked: false,
            }
        }

        pub fn delta(&self) -> TimeDelta {
            TimeDelta(self.target_delta)
        }

        pub fn update(&mut self) {
            let now = instant::Instant::now();
            let diff = now - self.last_tick;

            self.last_tick = now;
            self.accumulated_delta += diff;
            self.has_ticked = false;
        }

        pub fn tick(&mut self) -> bool {
            if self.accumulated_delta >= self.target_delta {
                self.accumulated_delta -= self.target_delta;
                self.has_ticked = true;

                true
            } else {
                false
            }
        }

        pub fn has_ticked(&self) -> bool {
            self.has_ticked
        }

        pub fn next_tick_proximity(&self) -> f32 {
            let delta = self.accumulated_delta;

            f32::from(self.target_ticks)
                * (delta.as_secs() as f32 + (delta.subsec_micros() as f32 / 1_000_000.0))
        }
    }
}
