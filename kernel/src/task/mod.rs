pub mod executor;
pub mod timer;

pub use executor::spawn;

pub fn start() {
    let mut executor = executor::Executor::new();

    executor::spawn(async {
        use crate::drivers::i8042::KeyStream;
        use futures_util::StreamExt;
        use i8042::DecodedKey;

        let mut keys = KeyStream::new();

        while let Some(key) = keys.next().await {
            match key {
                DecodedKey::Unicode(c) => {
                    print!("{c}");
                }
                DecodedKey::RawKey(_key) => {}
            }
        }
    });

    println!("[TASK] running");
    executor.run();
}

pub fn ap_start(ap_id: u8) {
    let mut executor = executor::Executor::new();
    println!("[TASK] added processor {ap_id}");
    executor.run();
}
