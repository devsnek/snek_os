#![no_std]

extern crate alloc;

use alloc::alloc::{GlobalAlloc, Layout};
use linked_list_allocator::Heap as LinkedListAllocator;
use spin::Mutex;

/// A memory allocator which uses a combination of
/// slab allocation and a fallback allocator.
pub struct Allocator {
    inner: Mutex<Inner>,
}

struct Inner {
    start: usize,
    size: usize,
    slabs: [SlabAllocator; 7],
    fallback: LinkedListAllocator,
}

impl Inner {
    fn slab(&mut self, layout: Layout) -> Option<&mut SlabAllocator> {
        let needed = layout.size().max(layout.align());
        self.slabs.iter_mut().find(|slab| slab.size >= needed)
    }
}

impl Allocator {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                start: 0,
                size: 0,
                slabs: [
                    SlabAllocator::new(32),
                    SlabAllocator::new(64),
                    SlabAllocator::new(128),
                    SlabAllocator::new(256),
                    SlabAllocator::new(512),
                    SlabAllocator::new(1024),
                    SlabAllocator::new(2048),
                ],
                fallback: LinkedListAllocator::empty(),
            }),
        }
    }

    pub fn init(&self, start: usize, size: usize) {
        let mut inner = self.inner.lock();
        inner.start = start;
        inner.size = size;
        let region_size = size / (inner.slabs.len() + 1);
        for (i, slab) in inner.slabs.iter_mut().enumerate() {
            let start = start + (region_size * i);
            slab.init(start, region_size);
        }
        let start = start + (region_size * inner.slabs.len());
        unsafe {
            inner.fallback.init(start as _, region_size);
        }
    }
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut inner = self.inner.lock();
        if let Some(slab) = inner.slab(layout) {
            slab.alloc(layout)
        } else {
            match inner.fallback.allocate_first_fit(layout).ok() {
                Some(allocation) => allocation.as_ptr(),
                None => core::ptr::null_mut(),
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut inner = self.inner.lock();
        if let Some(slab) = inner.slab(layout) {
            slab.dealloc(ptr, layout)
        } else {
            if ptr.is_null() {
                return;
            }
            inner
                .fallback
                .deallocate(core::ptr::NonNull::new_unchecked(ptr), layout)
        }
    }
}

struct SlabCell {
    next: usize,
}

#[derive(Debug)]
struct SlabAllocator {
    start: usize,
    end: usize,
    next: usize,
    size: usize,
}

impl SlabAllocator {
    const fn new(size: usize) -> Self {
        Self {
            start: 0,
            end: 0,
            next: 0,
            size,
        }
    }

    fn init(&mut self, start: usize, size: usize) {
        self.start = start;
        self.end = start + size;
        self.next = start;
    }

    unsafe fn alloc(&mut self, layout: Layout) -> *mut u8 {
        debug_assert!(layout.size() <= self.size);
        debug_assert!(layout.align() <= self.size);

        if self.next >= self.end {
            return core::ptr::null_mut();
        }

        let cell = self.next as *mut SlabCell;
        let cell_next = (*cell).next;
        (*cell).next = 0;
        self.next = if cell_next == 0 {
            self.next + self.size
        } else {
            cell_next
        };

        cell as *mut u8
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        debug_assert!(layout.size() <= self.size);
        debug_assert!(layout.align() <= self.size);

        let cell = ptr as *mut SlabCell;
        (*cell).next = self.next;
        self.next = cell as usize;
    }
}
