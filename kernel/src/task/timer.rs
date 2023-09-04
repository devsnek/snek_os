use crate::arch::TIMER_INTERVAL;
use maitake::time::{set_global_timer, Timer};

pub static TIMER: Timer = Timer::new(TIMER_INTERVAL);

pub fn init() {
    set_global_timer(&TIMER).unwrap();
}

pub fn on_tick() {
    TIMER.pend_ticks(1);
}
