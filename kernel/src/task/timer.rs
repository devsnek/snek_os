use core::time::Duration;
use maitake::time::{set_global_timer, Timer};

pub const TIMER_INTERVAL: Duration = Duration::from_millis(1);

pub static TIMER: Timer = Timer::new(TIMER_INTERVAL);

pub fn init() {
    set_global_timer(&TIMER).unwrap();
}

pub fn on_tick() {
    TIMER.pend_ticks(1);
}
