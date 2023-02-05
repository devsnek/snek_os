#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(prelude_import)]
#![feature(naked_functions)]
#![feature(const_mut_refs)]

#[macro_use]
extern crate lazy_static;
extern crate alloc;

mod prelude {
    pub use alloc::{
        borrow::ToOwned,
        boxed::Box,
        string::{String, ToString},
        vec::Vec,
    };
    pub use core::arch::asm;
    pub use core::prelude::v1::*;
}

#[prelude_import]
#[allow(unused_imports)]
use prelude::*;

#[macro_use]
mod debug;
mod arch;
mod task;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("panic: {}", info);
    arch::halt_loop();
}

#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout);
}

fn test_embedded_graphics() {
        use core::ops::DerefMut;
        use embedded_graphics::{
            pixelcolor::Rgb888,
            prelude::*,
            primitives::{PrimitiveStyle, Triangle},
        };

        let thin_stroke = PrimitiveStyle::with_stroke(Rgb888::new(255, 255, 255), 1);
        let yoffset = 10;
        Triangle::new(
            Point::new(16, 16 + yoffset),
            Point::new(16 + 16, 16 + yoffset),
            Point::new(16 + 8, yoffset),
        )
        .into_styled(thin_stroke)
        .draw(arch::DISPLAY.lock().deref_mut())
        .unwrap();
}

pub(crate) fn main() -> ! {
    println!("Welcome to SNEK OS");

    test_embedded_graphics();

    task::start();
}
