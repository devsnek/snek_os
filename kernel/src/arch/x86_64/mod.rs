mod acpi;
mod allocator;
mod framebuffer;
mod gdt;
mod interrupts;
mod memory;
mod pit;
mod smp;

use bootloader_api::{BootInfo, BootloaderConfig};
use x86_64::PhysAddr;

const CONFIG: BootloaderConfig = {
    use bootloader_api::config::*;

    let mut mappings = Mappings::new_default();
    mappings.kernel_stack = Mapping::Dynamic;
    mappings.boot_info = Mapping::Dynamic;
    mappings.framebuffer = Mapping::Dynamic;
    mappings.physical_memory = Some(Mapping::Dynamic);
    mappings.page_table_recursive = None;
    mappings.aslr = true;
    mappings.dynamic_range_start = Some(0xFFFF_8000_0000_0000);
    mappings.dynamic_range_end = Some(0xFFFF_FFFF_FFFF_FFFF);

    let frame_buffer = FrameBuffer::new_default();

    let mut config = BootloaderConfig::new_default();
    config.mappings = mappings;
    config.frame_buffer = frame_buffer;
    config.kernel_stack_size = 80 * 1024 * 128;
    config
};

fn kernel_start(boot_info: &'static mut BootInfo) -> ! {
    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        framebuffer::init(framebuffer.info(), framebuffer.buffer_mut());
    }

    gdt::init();

    allocator::init(
        boot_info.physical_memory_offset.into(),
        &mut boot_info.memory_regions,
    );

    let acpi_platform_info = acpi::init(PhysAddr::new(boot_info.rsdp_addr.into_option().unwrap()));

    interrupts::init(&acpi_platform_info);

    smp::init(&acpi_platform_info);

    crate::main();
}

bootloader_api::entry_point!(kernel_start, config = &CONFIG);

pub use framebuffer::_print;

#[inline(always)]
pub fn halt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

#[inline(always)]
pub fn enable_interrupts_and_halt() {
    x86_64::instructions::interrupts::enable_and_hlt();
}

#[inline(always)]
pub fn without_interrupts<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    x86_64::instructions::interrupts::without_interrupts(f)
}
