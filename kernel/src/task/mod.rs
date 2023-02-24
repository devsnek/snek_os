pub mod executor;
pub mod timer;

pub use executor::{spawn, spawn_blocking};

pub fn start() {
    let mut executor = executor::Executor::new();

    executor::spawn(crate::drivers::i8042::dispatch_keypresses());

    println!("[TASK] running");
    executor.run();
}
