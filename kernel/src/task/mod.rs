pub mod executor;
pub mod timer;

pub use executor::{block_on, spawn};

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
