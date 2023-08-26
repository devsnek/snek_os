use super::stack_allocator::StackAllocator;
use limine::{MemmapEntry, MemoryMapEntryType, NonNullPtr};
use spin::Mutex;
use x86_64::{
    registers::model_specific::Msr,
    structures::paging::{
        page::{PageRange, Size4KiB},
        FrameAllocator, Mapper, OffsetPageTable, Page, PageSize, PageTable, PageTableFlags,
        PhysFrame, Translate,
    },
    PhysAddr, VirtAddr,
};

use os_units::{Bytes, NumOfPages};

lazy_static! {
    pub static ref MAPPER: Mutex<Option<OffsetPageTable<'static>>> = Mutex::new(None);
    pub static ref FRAME_ALLOCATOR_ALLOCATOR: StackAllocator::<2048> = StackAllocator::new();
    pub static ref FRAME_ALLOCATOR: Mutex<Option<BootInfoFrameAllocator>> = Mutex::new(None);
}

pub fn init(physical_memory_offset: u64, memory_regions: &'static [NonNullPtr<MemmapEntry>]) {
    let level_4_table = unsafe { active_level_4_table(physical_memory_offset) };
    let table =
        unsafe { OffsetPageTable::new(level_4_table, VirtAddr::new(physical_memory_offset)) };

    let _ = MAPPER.lock().insert(table);

    let frame_allocator = unsafe { BootInfoFrameAllocator::init(memory_regions) };

    let _ = FRAME_ALLOCATOR.lock().insert(frame_allocator);

    init_pat();

    println!("[MEMORY] initialized");
}

unsafe fn active_level_4_table(physical_memory_offset: u64) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;

    let (level_4_table_frame, _) = Cr3::read();

    let phys = level_4_table_frame.start_address();
    let virt = VirtAddr::new(physical_memory_offset + phys.as_u64());
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    &mut *page_table_ptr // unsafe
}

pub struct BootInfoFrameAllocator {
    iter: Box<dyn core::iter::Iterator<Item = PhysFrame>, &'static StackAllocator<2048>>,
    free_frames: *mut FreePhysFrame,
}
unsafe impl Send for BootInfoFrameAllocator {}

impl BootInfoFrameAllocator {
    pub unsafe fn init(memory_regions: &'static [NonNullPtr<MemmapEntry>]) -> Self {
        let iter = Box::new_in(
            memory_regions
                .iter()
                .map(|p| &*p.as_ptr())
                .filter(|r| r.typ == MemoryMapEntryType::Usable)
                .map(|r| r.base..(r.base + r.len))
                .flat_map(|r| r.step_by(4096))
                .map(|addr| PhysFrame::containing_address(PhysAddr::new(addr))),
            &*FRAME_ALLOCATOR_ALLOCATOR,
        );

        BootInfoFrameAllocator {
            iter,
            free_frames: core::ptr::null_mut(),
        }
    }

    #[allow(unused)]
    pub fn deallocate_frame(&mut self, frame: PhysFrame) {
        let next = self.free_frames;
        let free_frame = FRAME_ALLOCATOR_ALLOCATOR.own(FreePhysFrame { frame, next });
        self.free_frames = free_frame;
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        if !self.free_frames.is_null() {
            unsafe {
                let frame = (*self.free_frames).frame;
                self.free_frames = (*self.free_frames).next;
                return Some(frame);
            }
        }
        self.iter.next()
    }
}

struct FreePhysFrame {
    frame: PhysFrame,
    next: *mut FreePhysFrame,
}

fn search_free_addr_from(num_pages: NumOfPages<Size4KiB>, region: PageRange) -> Option<VirtAddr> {
    let mut cnt = 0;
    let mut start = None;
    for page in region {
        let addr = page.start_address();
        if available(addr) {
            if start.is_none() {
                start = Some(addr);
            }

            cnt += 1;

            if cnt >= num_pages.as_usize() {
                return start;
            }
        } else {
            cnt = 0;
            start = None;
        }
    }

    None
}

fn available(addr: VirtAddr) -> bool {
    let mut binding = super::memory::MAPPER.lock();
    let mapper = binding.as_mut().unwrap();

    mapper.translate_addr(addr).is_none() && !addr.is_null()
}

pub fn map_pages_from(start: PhysAddr, object_size: usize, region: PageRange) -> VirtAddr {
    let start_frame_addr = start.align_down(Size4KiB::SIZE);
    let end_frame_addr = (start + object_size).align_down(Size4KiB::SIZE);

    let num_pages =
        Bytes::new((end_frame_addr - start_frame_addr) as usize + 1).as_num_of_pages::<Size4KiB>();

    let virt = search_free_addr_from(num_pages, region)
        .expect("OOM during creating a new accessor to a register.");

    let mut mapper = super::memory::MAPPER.lock();
    let mapper = mapper.as_mut().unwrap();

    let mut frame_allocator = super::memory::FRAME_ALLOCATOR.lock();
    let frame_allocator = frame_allocator.as_mut().unwrap();

    for i in 0..num_pages.as_usize() {
        let page = Page::<Size4KiB>::containing_address(virt + Size4KiB::SIZE * i as u64);
        let frame = PhysFrame::containing_address(start_frame_addr + Size4KiB::SIZE * i as u64);
        let flag =
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

        unsafe {
            mapper
                .map_to(page, frame, flag, frame_allocator)
                .unwrap()
                .flush();
        }
    }

    let page_offset = start.as_u64() % Size4KiB::SIZE;

    virt + page_offset
}

pub fn map_address(phys: PhysAddr, size: usize) -> VirtAddr {
    map_pages_from(
        phys,
        size,
        PageRange {
            start: Page::from_start_address(VirtAddr::new(0xFFFF_8000_0000_0000)).unwrap(),
            end: Page::containing_address(VirtAddr::new(0xFFFF_FFFF_FFFF_FFFF)),
        },
    )
}

pub fn translate_addr(addr: VirtAddr) -> Option<PhysAddr> {
    let mut binding = super::memory::MAPPER.lock();
    let mapper = binding.as_mut().unwrap();

    mapper.translate_addr(addr)
}

#[derive(Debug, Copy, Clone)]
#[repr(u8)]
enum MemoryCachingType {
    Uncacheable = 0x00,
    WriteCombining = 0x01,
    WriteThrough = 0x04,
    WriteProtected = 0x05,
    WriteBack = 0x06,
    UncachedMinus = 0x07,
}

mycelium_bitfield::bitfield! {
    struct PageAttributeTable<u64> {
        const SLOT_0 = 3;
        const _RESERVED_0 = 5;
        const SLOT_1 = 3;
        const _RESERVED_1 = 5;
        const SLOT_2 = 3;
        const _RESERVED_2 = 5;
        const SLOT_3 = 3;
        const _RESERVED_3 = 5;
        const SLOT_4 = 3;
        const _RESERVED_4 = 5;
        const SLOT_5 = 3;
        const _RESERVED_5 = 5;
        const SLOT_6 = 3;
        const _RESERVED_6 = 5;
        const SLOT_7 = 3;
        const _RESERVED_7 = 5;
    }
}

fn init_pat() {
    let pat = PageAttributeTable::new()
        .with(
            PageAttributeTable::SLOT_0,
            MemoryCachingType::WriteBack as _,
        )
        .with(
            PageAttributeTable::SLOT_1,
            MemoryCachingType::WriteThrough as _,
        )
        .with(
            PageAttributeTable::SLOT_2,
            MemoryCachingType::Uncacheable as _,
        )
        .with(
            PageAttributeTable::SLOT_3,
            MemoryCachingType::Uncacheable as _,
        )
        .with(
            PageAttributeTable::SLOT_4,
            MemoryCachingType::WriteProtected as _,
        )
        .with(
            PageAttributeTable::SLOT_5,
            MemoryCachingType::WriteCombining as _,
        )
        .with(
            PageAttributeTable::SLOT_6,
            MemoryCachingType::UncachedMinus as _,
        )
        .with(
            PageAttributeTable::SLOT_7,
            MemoryCachingType::UncachedMinus as _,
        );

    let mut msr = Msr::new(0x277);
    unsafe {
        msr.write(pat.into());
    }
}

pub(crate) fn set_page_flags<T: PageSize>(page: Page<T>, flags: PageTableFlags)
    where OffsetPageTable<'static>: Mapper<T>
{
    let mut binding = super::memory::MAPPER.lock();
    let mapper = binding.as_mut().unwrap();

    unsafe {
        if let Err(err) = mapper.update_flags(page, flags) {
            drop(binding);
            panic!("{:?}", err);
        }
    }
}
