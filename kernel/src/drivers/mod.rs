pub mod i8042;
// pub mod xhci;

pub fn init() {
    i8042::init();
    // xhci::init();

    println!("[DRIVERS] initialized");
}
