use core::{
    alloc::{AllocError, Allocator, Layout},
    ptr::NonNull,
};
use spin::Mutex;

pub struct StackAllocator<const SIZE: usize> {
    inner: Mutex<StackAllocatorInner<SIZE>>,
}

impl<const SIZE: usize> core::fmt::Debug for StackAllocator<SIZE> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "StackAllocator {{ size: {SIZE} }}")
    }
}

struct StackAllocatorInner<const SIZE: usize> {
    memory: [u8; SIZE],
    current: usize,
}

impl<const SIZE: usize> StackAllocator<SIZE> {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(StackAllocatorInner {
                memory: [0; SIZE],
                current: 0,
            }),
        }
    }

    #[allow(clippy::mut_from_ref)]
    pub fn own<T: Sized>(&self, value: T) -> &mut T {
        let ptr = self.allocate(Layout::new::<T>()).unwrap().as_mut_ptr() as *mut T;
        unsafe {
            core::ptr::write(ptr, value);
            &mut *ptr
        }
    }

    #[allow(unused)]
    pub fn used(&self) -> usize {
        self.inner.lock().current
    }
}

unsafe impl<const SIZE: usize> Allocator for StackAllocator<SIZE> {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let mut inner = self.inner.lock();
        if inner.current + layout.size() >= inner.memory.len() {
            return Err(AllocError);
        }
        let ptr = &inner.memory[inner.current] as *const u8 as *mut ();
        let ptr = core::ptr::from_raw_parts_mut(ptr, layout.size());
        inner.current += layout.size();
        Ok(unsafe { NonNull::new_unchecked(ptr) })
    }

    unsafe fn deallocate(&self, _ptr: NonNull<u8>, _layout: Layout) {}
}
