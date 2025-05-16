use crate::arch::TIMER_INTERVAL;
use core::sync::atomic::{AtomicU64, Ordering};
use maitake::time::{set_global_timer, Clock, Timer};

static CLOCK_TICKS: AtomicU64 = AtomicU64::new(0);

pub static TIMER: Timer = Timer::new(Clock::new(TIMER_INTERVAL, || {
    CLOCK_TICKS.load(Ordering::Relaxed)
}));

pub fn init() {
    set_global_timer(&TIMER).unwrap();
}

pub fn on_tick() {
    CLOCK_TICKS.fetch_add(1, Ordering::Relaxed);
}
