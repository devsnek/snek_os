#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(prelude_import)]
#![feature(naked_functions)]
#![feature(const_mut_refs)]
#![feature(never_type)]
#![feature(asm_const)]

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
    pub use core::arch::{asm, global_asm};
    pub use core::prelude::v1::*;
}

#[prelude_import]
#[allow(unused_imports)]
use prelude::*;

#[macro_use]
mod debug;
mod arch;
mod drivers;
mod task;
mod util;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("panic: {}", info);

    arch::halt_loop();
}

#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout);
}

pub fn main() -> ! {
    println!("Welcome to SNEK OS");

    drivers::init();

    task::start();

    println!("tasks finished?");

    arch::halt_loop();
}

pub struct ProcessorInfo {
    id: u32,
}

pub fn ap_main(info: ProcessorInfo) -> ! {
    println!("[SMP] processor {} started", info.id);

    arch::halt_loop();
}
