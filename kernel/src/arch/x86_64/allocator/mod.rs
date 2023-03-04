mod block_allocator;
use block_allocator::BlockAllocator;

use x86_64::{
    structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, Translate},
    VirtAddr,
};

pub const HEAP_START: usize = 0x4444_4444_0000;
pub const HEAP_END: usize = 0xFFFF_8000_0000_0000;

#[global_allocator]
pub static ALLOCATOR: Locked<BlockAllocator> = Locked::new(BlockAllocator::new());

pub fn lazy_map(address: VirtAddr) -> bool {
    if address.as_u64() < HEAP_START as u64 || address.as_u64() >= HEAP_END as u64 {
        return false;
    }

    let mut binding = super::memory::MAPPER.lock();
    let mapper = binding.as_mut().unwrap();

    if mapper.translate_addr(address).is_some() {
        return false;
    }

    let mut binding = super::memory::FRAME_ALLOCATOR.lock();
    let frame_allocator = binding.as_mut().unwrap();

    let page = Page::containing_address(address);

    let frame = frame_allocator.allocate_frame().unwrap();
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
    unsafe {
        mapper
            .map_to(page, frame, flags, frame_allocator)
            .unwrap()
            .flush()
    };

    true
}

pub fn init() {
    // manually map a page since interrupts may not be enabled yet
    assert!(lazy_map(VirtAddr::new(HEAP_START as u64)));

    unsafe { ALLOCATOR.lock().init(HEAP_START, HEAP_END) };

    println!("[ALLOCATOR] initialized");
}

// wrapper for GlobalAlloc
pub struct Locked<T> {
    inner: spin::Mutex<T>,
}

impl<T> Locked<T> {
    pub const fn new(inner: T) -> Self {
        Locked {
            inner: spin::Mutex::new(inner),
        }
    }

    pub fn lock(&self) -> spin::MutexGuard<T> {
        self.inner.lock()
    }
}
