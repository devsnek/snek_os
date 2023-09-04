pub mod executor;
pub mod timer;

pub use executor::spawn;

pub fn start(_ap_id: u8) {
    let mut executor = executor::Executor::new();
    executor.run();
}
