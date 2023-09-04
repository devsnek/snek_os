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
    if line == "timing" {
        crate::task::spawn(async {
            for _ in 0..30 {
                let uptime = crate::arch::now();
                let unix = crate::arch::timestamp();
                println!("{uptime:?} {unix:?}");
                maitake::time::sleep(core::time::Duration::from_secs(1)).await;
            }
        });
        return;
    }
    if line == "nettest" {
        crate::net::test_task();
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
