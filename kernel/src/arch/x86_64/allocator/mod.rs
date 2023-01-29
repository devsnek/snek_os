use x86_64::{
    structures::paging::{
        mapper::MapToError, FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB,
    },
    VirtAddr,
};

mod block_allocator;
use block_allocator::BlockAllocator;

pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const HEAP_SIZE: usize = 1 * 1024 * 1024;

#[global_allocator]
static ALLOCATOR: Locked<BlockAllocator> = Locked::new(BlockAllocator::new());

fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    println!("[HEAP] initializing");

    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    dbg!(page_range);

    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe { mapper.map_to(page, frame, flags, frame_allocator)?.flush() };
    }

    unsafe { ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE) };

    println!("[HEAP] initialized");

    Ok(())
}

pub fn init(
    physical_memory_offset: Option<u64>,
    memory_regions: &'static mut bootloader_api::info::MemoryRegions,
) {
    println!("[ALLOCATOR] initializing");

    let mut mapper = unsafe { super::memory::init(physical_memory_offset.unwrap_or(0)) };
    let mut frame_allocator =
        unsafe { super::memory::BootInfoFrameAllocator::init(memory_regions) };
    init_heap(&mut mapper, &mut frame_allocator).unwrap();

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
