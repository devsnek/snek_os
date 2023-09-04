use super::interrupts::TIMER_INTERVAL;
use chrono::{TimeZone, Utc};
use cmos::CMOS;
use core::{
    arch::x86_64::_rdtsc as rdtsc,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

static UPTIME_MS: AtomicU64 = AtomicU64::new(0);
static BOOT_SEC: AtomicU64 = AtomicU64::new(0);
static LAST_RTC: AtomicU64 = AtomicU64::new(0);

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
    let tsc = unsafe { rdtsc() };
    LAST_RTC.store(tsc, Ordering::Relaxed);
}

pub fn now() -> Duration {
    Duration::from_millis(UPTIME_MS.load(Ordering::SeqCst))
}

pub fn timestamp() -> Duration {
    Duration::from_secs(BOOT_SEC.load(Ordering::SeqCst)) + now()
}
