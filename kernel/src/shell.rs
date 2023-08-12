use crate::drivers::i8042::next_key;
use i8042::{DecodedKey, KeyCode};

fn run(line: &str) {
    if line == "shutdown" {
        crate::arch::shutdown();
        return;
    }
    if line == "panic" {
        crate::task::spawn(async {
            panic!("a panic!");
        });
        return;
    }
    if line == "loop" {
        crate::task::spawn(async {
            #[allow(clippy::empty_loop)]
            loop {}
        });
        return;
    }
    println!("unknown command");
}

pub async fn shell() {
    let mut line = String::new();
    print!("> ");
    while let Some(key) = next_key().await {
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
