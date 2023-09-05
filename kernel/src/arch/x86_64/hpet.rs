use core::sync::atomic::{AtomicPtr, Ordering};
use hpet::Hpet;
use x86_64::PhysAddr;

static HPET: AtomicPtr<Hpet> = AtomicPtr::new(core::ptr::null_mut());

fn get_hpet() -> Option<&'static mut Hpet> {
    let hpet = HPET.load(Ordering::SeqCst);
    if hpet.is_null() {
        None
    } else {
        Some(unsafe { &mut *hpet })
    }
}

pub fn get_counter() -> Option<u64> {
    get_hpet().map(|h| h.counter_value())
}

pub fn get_counter_period() -> Option<u32> {
    get_hpet().map(|h| h.counter_period())
}

pub fn init() {
    let Some(hpet_info) = super::acpi::get_hpet() else {
        return;
    };

    let hpet = super::memory::map_address(
        PhysAddr::new(hpet_info.base_address as u64),
        core::mem::size_of::<Hpet>(),
    );

    HPET.store(hpet.as_u64() as _, Ordering::Relaxed);

    get_hpet().unwrap().set_counter_enable(true);
}
