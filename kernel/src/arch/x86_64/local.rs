use alloc::collections::BTreeMap;
use core::{marker::PhantomPinned, pin::Pin};
use spin::Mutex;
use x86_64::{
    registers::model_specific::{GsBase, KernelGsBase},
    VirtAddr,
};

#[repr(C)]
#[derive(Debug)]
pub struct GsLocalData {
    _self: *const Self,
    magic: usize,
    pub data: Mutex<BTreeMap<usize, *mut ()>>,
    _must_pin: PhantomPinned,
}

impl GsLocalData {
    const MAGIC: usize = 0xDEADBEEF;
    fn new() -> Self {
        Self {
            _self: core::ptr::null(),
            magic: Self::MAGIC,
            data: Mutex::new(BTreeMap::new()),
            _must_pin: PhantomPinned,
        }
    }

    pub fn get() -> Option<Pin<&'static Self>> {
        let this = unsafe {
            let ptr: *const Self;
            asm!("mov {}, gs:0x0", out(reg) ptr);
            if ptr.is_null() {
                return None;
            }
            Pin::new_unchecked(&*ptr)
        };
        if this.magic != Self::MAGIC {
            return None;
        }
        Some(this)
    }
}

pub fn init() {
    let ptr = Box::into_raw(Box::new(GsLocalData::new()));
    unsafe {
        (*ptr)._self = ptr as *const _;
        GsBase::write(VirtAddr::new(ptr as u64));
        KernelGsBase::write(VirtAddr::new(ptr as u64));
    }

    debug!("[LOCAL] initialized");
}
