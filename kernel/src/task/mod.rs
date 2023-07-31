pub mod executor;
pub mod timer;

pub use executor::spawn;

pub fn start(ap_id: u8) {
    let mut executor = executor::Executor::new();
    println!("[TASK] added processor {ap_id}");
    executor.run();
}
