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

pub(crate) fn main() -> ! {
    println!("Welcome to SNEK OS");

    task::start();
}
