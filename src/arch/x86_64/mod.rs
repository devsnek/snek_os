mod allocator;
mod gdt;
mod interrupts;
mod memory;
mod mode;

fn kernel_start(boot_info: &'static mut bootloader::BootInfo) -> ! {
    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        crate::framebuffer::init(framebuffer.info(), framebuffer.buffer_mut());
    }

    gdt::init();
    interrupts::init();
    allocator::init(boot_info.physical_memory_offset.into(), &mut boot_info.memory_regions);

    crate::main();
}

bootloader::entry_point!(kernel_start);

#[inline(always)]
pub fn halt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

#[inline(always)]
pub fn enable_interrupts() {
    x86_64::instructions::interrupts::enable();
}

#[inline(always)]
pub fn disable_interrupts() {
    x86_64::instructions::interrupts::disable();
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
