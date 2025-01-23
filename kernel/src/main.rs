#![no_std]
#![no_main]
#![allow(internal_features)]
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
#![feature(panic_can_unwind)]
#![feature(strict_provenance)]

#[macro_use]
extern crate lazy_static;
extern crate alloc;

mod prelude {
    #![allow(unused)]
    pub use alloc::{
        borrow::ToOwned,
        boxed::Box,
        format,
        string::{String, ToString},
        vec,
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
mod net;
mod panic;
mod shell;
mod task;
mod wasm;

pub fn main() -> ! {
    println!("Welcome to snek_os");

    drivers::init();

    task::spawn(shell::shell());

    task::spawn(async {
        arch::init_smp();
    });

    ap_main(0);
}

pub fn ap_main(ap_id: u8) -> ! {
    task::start(ap_id);
    arch::halt_loop();
}
