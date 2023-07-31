use core::panic::PanicInfo;
use rustc_demangle::demangle;

static mut IN_PANIC: bool = false;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    crate::arch::disable_interrupts();

    unsafe {
        if IN_PANIC {
            e9::println!("PANIC IN PANIC: {}", info);
            println!("PANIC IN PANIC: {}", info);
            crate::arch::halt_loop();
        }
        IN_PANIC = true;
    }

    e9::println!("PANIC: {}", info);
    println!("PANIC: {}", info);

    crate::arch::stack_trace(16, |frame| {
        if let Some((f_addr, f_name)) = frame.function {
            println!(
                "  at 0x{:016x} {:#}+0x{:x}",
                frame.address,
                demangle(f_name),
                frame.address - f_addr
            );
        } else {
            println!("  at 0x{:016x}", frame.address);
        }
    });

    crate::arch::halt_loop();
}
