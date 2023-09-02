use chrono::{TimeZone, Utc};
use cmos::CMOS;
use core::sync::atomic::{AtomicI64, AtomicU64, Ordering};

static UPTIME_MS: AtomicU64 = AtomicU64::new(0);
static BOOT_SEC: AtomicI64 = AtomicI64::new(0);

pub fn init() {
    let mut cmos = CMOS::new();
    let rtc = cmos.read_rtc(super::acpi::get_century_register());
    BOOT_SEC.store(Utc.from_utc_datetime(&rtc).timestamp(), Ordering::Relaxed);
}

pub fn on_tick() {
    UPTIME_MS.fetch_add(1, Ordering::Relaxed);
}

pub fn now() -> u64 {
    UPTIME_MS.load(Ordering::SeqCst) * 1000
}
