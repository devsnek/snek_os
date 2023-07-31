use crate::drivers::i8042::KeyStream;
use futures_util::StreamExt;
use i8042::{DecodedKey, KeyCode};

fn run(line: &str) {
    if line == "shutdown" {
        crate::arch::shutdown();
        return;
    }
    println!("unknown command");
}

pub async fn shell() {
    let mut keys = KeyStream::new();

    let mut line = String::new();
    print!("> ");
    while let Some(key) = keys.next().await {
        match key {
            DecodedKey::Unicode('\r') | DecodedKey::Unicode('\n') => {
                print!("\n");
                run(&line);
                line.clear();
                print!("> ");
            }
            DecodedKey::RawKey(KeyCode::Backspace)
            | DecodedKey::RawKey(KeyCode::Delete)
            | DecodedKey::Unicode('\u{0008}') => {
                line.pop();
            }
            DecodedKey::Unicode(c) => {
                print!("{c}");
                line.push(c);
            }
            DecodedKey::RawKey(_key) => {}
        }
    }
}
