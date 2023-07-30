#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(prelude_import)]
#![feature(naked_functions)]
#![feature(const_mut_refs)]
#![feature(never_type)]
#![feature(asm_const)]
#![feature(allocator_api)]
#![feature(ptr_metadata)]
#![feature(slice_ptr_get)]
#![feature(inline_const)]

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
mod panic;
mod task;

pub fn main() -> ! {
    println!("Welcome to snek_os");

    drivers::init();

    arch::init_smp();

    task::start();

    arch::halt_loop();
}

pub fn ap_main(ap_id: u8) -> ! {
    task::ap_start(ap_id);

    arch::halt_loop();
}
