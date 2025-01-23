use super::interrupts::TIMER_INTERVAL;
use chrono::{TimeZone, Utc};
use cmos::CMOS;
use core::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

static UPTIME_MS: AtomicU64 = AtomicU64::new(0);
static BOOT_SEC: AtomicU64 = AtomicU64::new(0);
static LAST_HPET: AtomicU64 = AtomicU64::new(0);

pub fn init() {
    let mut cmos = CMOS::new();
    let rtc = cmos.read_rtc(super::acpi::get_century_register());
    BOOT_SEC.store(
        Utc.from_utc_datetime(&rtc).timestamp() as _,
        Ordering::Relaxed,
    );
}

pub fn on_tick() {
    UPTIME_MS.fetch_add(TIMER_INTERVAL.as_millis() as _, Ordering::Relaxed);
    if let Some(counter) = super::hpet::get_counter() {
        LAST_HPET.store(counter, Ordering::Relaxed);
    }
}

pub fn now() -> Duration {
    let counter = super::hpet::get_counter();
    let now = Duration::from_millis(UPTIME_MS.load(Ordering::SeqCst));
    if let Some(counter) = counter {
        let period = super::hpet::get_counter_period().unwrap();
        let last = LAST_HPET.load(Ordering::SeqCst);
        let fs = counter.saturating_sub(last) * period as u64;
        now + Duration::from_nanos(fs / 1000000)
    } else {
        now
    }
}

pub fn timestamp() -> Duration {
    Duration::from_secs(BOOT_SEC.load(Ordering::SeqCst)) + now()
}
