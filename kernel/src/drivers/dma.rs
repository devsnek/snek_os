use crate::arch::translate_virt_addr;
use alloc::alloc::{alloc_zeroed, dealloc, handle_alloc_error, Layout};
use unique::Unique;
use x86_64::VirtAddr;

pub struct Dma<T: ?Sized> {
    layout: Layout,
    dma: Unique<T>,
}

impl<T: Sized> Dma<T> {
    pub fn new_zeroed(align: usize) -> Dma<T> {
        assert!(align >= core::mem::align_of::<T>());
        let layout = Layout::new::<T>().align_to(align).unwrap();
        let ptr = unsafe { alloc_zeroed(layout) };
        if ptr.is_null() {
            handle_alloc_error(layout);
        }
        let dma = unsafe { Unique::new_unchecked(ptr as _) };
        Self { layout, dma }
    }

    pub fn as_ptr(&mut self) -> *mut T {
        self.dma.as_ptr()
    }
}

impl<T> Dma<[T]> {
    pub fn new_zeroed_slice(len: usize, align: usize) -> Dma<[T]> {
        assert!(align >= core::mem::align_of::<T>());
        let layout = Layout::array::<T>(len).unwrap().align_to(align).unwrap();
        let ptr = unsafe { alloc_zeroed(layout) };
        if ptr.is_null() {
            handle_alloc_error(layout);
        }
        let slice = unsafe { core::slice::from_raw_parts_mut(ptr as *mut T, len) };
        let dma = unsafe { Unique::new_unchecked(slice) };
        Self { layout, dma }
    }
}

impl<T: ?Sized> Dma<T> {
    pub fn phys_addr(&self) -> usize {
        translate_virt_addr(VirtAddr::new(self.dma.as_ptr() as *mut () as _))
            .unwrap()
            .as_u64() as _
    }

    pub fn leak(self) -> *mut T {
        let ptr = self.dma.as_ptr();
        core::mem::forget(self);
        ptr
    }
}

impl<T: ?Sized> Drop for Dma<T> {
    fn drop(&mut self) {
        unsafe {
            let ptr = self.dma.as_ptr();
            if !ptr.is_null() {
                core::ptr::drop_in_place(ptr);
                dealloc(ptr as _, self.layout);
            }
        }
    }
}

impl<T: ?Sized> core::ops::Deref for Dma<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { self.dma.as_ref() }
    }
}

impl<T: ?Sized> core::ops::DerefMut for Dma<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { self.dma.as_mut() }
    }
}

impl<T: ?Sized> core::fmt::Debug for Dma<T>
where
    T: core::fmt::Debug,
{
    fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
        unsafe { self.dma.as_ref().fmt(fmt) }
    }
}
