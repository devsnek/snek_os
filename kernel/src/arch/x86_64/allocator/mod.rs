use x86_64::{
    structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags},
    VirtAddr,
};

mod block_allocator;
use block_allocator::BlockAllocator;

pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const HEAP_SIZE: usize = 2 * 1024 * 1024;

#[global_allocator]
static ALLOCATOR: Locked<BlockAllocator> = Locked::new(BlockAllocator::new());

pub fn init(
    physical_memory_offset: Option<u64>,
    memory_regions: &'static mut bootloader_api::info::MemoryRegions,
) {
    println!("[ALLOCATOR] initializing");

    unsafe {
        super::memory::init(physical_memory_offset.unwrap_or(0), memory_regions);
    }

    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    let mut binding = super::memory::MAPPER.lock();
    let mapper = binding.as_mut().unwrap();
    let mut binding = super::memory::FRAME_ALLOCATOR.lock();
    let frame_allocator = binding.as_mut().unwrap();
    for page in page_range {
        let frame = frame_allocator.allocate_frame().unwrap();
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            mapper
                .map_to(page, frame, flags, frame_allocator)
                .unwrap()
                .flush()
        };
    }

    unsafe { ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE) };

    println!("[ALLOCATOR] initialized");
}

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
