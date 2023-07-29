pub mod i8042;

pub fn init() {
    i8042::init();

    println!("[DRIVERS] initialized");
}
