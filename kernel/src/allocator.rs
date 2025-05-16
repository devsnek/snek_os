use snalloc::Allocator;

pub const MANAGED_START: usize = 0x0000_1000_0000_0000;
pub const MANAGED_END: usize = 0x0000_7fff_ffff_f000;

#[global_allocator]
pub static ALLOCATOR: Allocator = Allocator::new();

pub fn init() {
    ALLOCATOR.init(MANAGED_START, MANAGED_END - MANAGED_START);

    debug!("[ALLOCATOR] initialized");
}
