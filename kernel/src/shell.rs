use crate::drivers::keyboard::{next_key, DecodedKey, KeyCode};

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
    if line.starts_with("timing") {
        let n = line
            .strip_prefix("timing ")
            .unwrap()
            .parse::<u32>()
            .unwrap();
        crate::task::spawn(async move {
            for _ in 0..n {
                let uptime = crate::arch::now();
                let unix = crate::arch::timestamp();
                println!("{uptime:?} {unix:?}");
                maitake::time::sleep(core::time::Duration::from_secs(1)).await;
            }
        });
        return;
    }
    if line.starts_with("nettest") {
        let host = line.strip_prefix("nettest ").unwrap();
        crate::net::test_task(host.to_string());
        return;
    }
    /*
    if line == "wasm" {
        let wasm =
            include_bytes!("../../programs/wasm_test/target/wasm32-wasi/release/wasm_test.wasm");
        crate::wasm::run(wasm);
        return;
    }
    */
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
