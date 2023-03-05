mod acpi;
mod allocator;
mod e9;
mod framebuffer;
mod gdt;
mod interrupts;
mod local;
mod memory;
mod pci;
mod pit;
mod stack_trace;
mod syscall;

use bootloader_api::{BootInfo, BootloaderConfig};
use x86_64::{PhysAddr, VirtAddr};

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

    let mut config = BootloaderConfig::new_default();
    config.mappings = mappings;
    config.kernel_stack_size = 80 * 1024 * 128;
    config
};

fn kernel_start(boot_info: &'static mut BootInfo) -> ! {
    e9::debug("hey there :)");

    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        framebuffer::init(framebuffer.info(), framebuffer.buffer_mut());
    }

    gdt::init();

    memory::init(
        boot_info.physical_memory_offset.into(),
        &mut boot_info.memory_regions,
    );

    stack_trace::init(
        unsafe {
            let virt_addr = VirtAddr::new(
                boot_info.kernel_addr + Option::from(boot_info.physical_memory_offset).unwrap_or(0),
            )
            .as_u64();
            core::slice::from_raw_parts(virt_addr as _, boot_info.kernel_len as _)
        },
        boot_info.kernel_image_offset as _,
    );

    allocator::init();

    let (acpi_platform_info, pci_regions, _implements_8042) =
        acpi::init(PhysAddr::new(boot_info.rsdp_addr.into_option().unwrap()));

    interrupts::init(&acpi_platform_info);

    syscall::init();

    local::init();

    pci::init(pci_regions, boot_info.physical_memory_offset.into());

    crate::main();
}

bootloader_api::entry_point!(kernel_start, config = &CONFIG);

pub use framebuffer::_print;
pub use local::Local;
pub use stack_trace::stack_trace;

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
