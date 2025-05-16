#![no_std]
#![no_main]
#![allow(internal_features)]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(prelude_import)]
#![feature(naked_functions)]
#![feature(never_type)]
#![feature(allocator_api)]
#![feature(ptr_metadata)]
#![feature(slice_ptr_get)]
#![feature(panic_can_unwind)]
#![feature(duration_millis_float)]

extern crate alloc;

#[macro_use]
extern crate tracing;

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

mod allocator;
mod arch;
mod debug;
mod drivers;
mod framebuffer;
mod local;
mod net;
mod panic;
mod shell;
mod stack_allocator;
mod task;

#[no_mangle]
unsafe extern "C" fn _start() -> ! {
    crate::panic::catch_unwind(start);
}

fn start() -> ! {
    debug::init();

    framebuffer::early_init();

    debug::set_print(framebuffer::print);

    arch::init();

    shell::start();

    drivers::init();

    ap_main(0);
}

pub fn ap_main(ap_id: u8) -> ! {
    task::start(ap_id);
    arch::halt_loop();
}
