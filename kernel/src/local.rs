use alloc::collections::btree_map::Entry;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Lazy;

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
        ID.fetch_add(1, Ordering::Relaxed)
    }

    pub fn with<U, F>(&self, f: F) -> U
    where
        F: FnOnce(&T) -> U,
    {
        let local = crate::arch::LocalData::get().expect("failed to load LocalData");
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
