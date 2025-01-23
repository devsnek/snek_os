#[cfg(target_arch = "x86_64")]
#[path = "x86_64/mod.rs"]
mod imp;

pub use self::imp::*;

mod common;

pub use common::*;
