use alloc::collections::{btree_map::Entry, BTreeMap};
use core::{
    marker::PhantomPinned,
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
};
use spin::{Lazy, Mutex};
use x86_64::{
    registers::model_specific::{GsBase, KernelGsBase},
    VirtAddr,
};

pub struct Local<T> {
    id: Lazy<usize>,
    initializer: fn() -> T,
}

impl<T: 'static> Local<T> {
    pub const fn new(initializer: fn() -> T) -> Self {
        Self {
            id: Lazy::new(Self::next_id),
            initializer,
        }
    }

    fn next_id() -> usize {
        static ID: AtomicUsize = AtomicUsize::new(0);
        let id = ID.fetch_add(1, Ordering::Relaxed);
        id
    }

    pub fn with<U, F>(&self, f: F) -> U
    where
        F: FnOnce(&T) -> U,
    {
        let local = GsLocalData::get().expect("failed to load GsLocalData");
        let ptr = match local.data.lock().entry(*self.id) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(v) => {
                let ptr = Box::into_raw(Box::new((self.initializer)())) as *mut ();
                v.insert(ptr);
                ptr
            }
        };
        let data = unsafe { &*(ptr as *const T) };
        f(data)
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct GsLocalData {
    _self: *const Self,
    magic: usize,
    data: Mutex<BTreeMap<usize, *mut ()>>,
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

    fn get() -> Option<Pin<&'static Self>> {
        if GsBase::read() == VirtAddr::new(0) {
            return None;
        }
        let magic: usize;
        unsafe {
            asm!("mov {}, gs:0x8", out(reg) magic);
        }
        if magic != Self::MAGIC {
            return None;
        }
        unsafe {
            let ptr: *const Self;
            asm!("mov {}, gs:0x0", out(reg) ptr);
            Some(Pin::new_unchecked(&*ptr))
        }
    }
}

pub fn init() {
    let ptr = Box::into_raw(Box::new(GsLocalData::new()));
    unsafe {
        (*ptr)._self = ptr as *const _;
        GsBase::write(VirtAddr::new(ptr as u64));
        KernelGsBase::write(VirtAddr::new(ptr as u64));
    }
}
