use snalloc::Allocator;
use x86_64::{
    structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, Translate},
    VirtAddr,
};

pub const HEAP_START: usize = 0x4000_0000_0000;
pub const HEAP_END: usize = 0x8000_0000_0000;

#[global_allocator]
pub static ALLOCATOR: Allocator = Allocator::new();

pub fn init() {
    ALLOCATOR.init(HEAP_START, HEAP_END - HEAP_START);

    println!("[ALLOCATOR] initialized");
}

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

    super::interrupts::send_flush_tlb();

    unsafe {
        core::slice::from_raw_parts_mut(page.start_address().as_u64() as *mut u8, page.size() as _)
            .fill(0);
    }

    true
}
