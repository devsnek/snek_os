use core::panic::PanicInfo;
use rustc_demangle::demangle;

static mut IN_PANIC: bool = false;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe {
        if IN_PANIC {
            println!("PANIC IN PANIC: {}", info);
            crate::arch::halt_loop();
        }
        IN_PANIC = true;
    }

    crate::arch::without_interrupts(|| -> ! {
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
    });
}
