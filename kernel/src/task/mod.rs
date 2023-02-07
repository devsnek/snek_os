pub mod executor;
pub mod keyboard;
pub mod timer;

pub use executor::{spawn, spawn_blocking};

pub fn start() {
    let mut executor = executor::Executor::new();

    executor::spawn(keyboard::dispatch_keypresses());

    println!("[TASK] running");
    executor.run();
}
