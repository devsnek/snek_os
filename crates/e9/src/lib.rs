#![no_std]

pub fn detect() -> bool {
    let mut b: u8;
    unsafe {
        core::arch::asm!(r#"
        in al, 0xe9
        "#, out("al") b);
    }
    b == 0xe9
}

pub fn write_byte(b: u8) {
    if b == b'\n' {
        write_byte(b'\r');
    }
    unsafe {
        core::arch::asm!(r#"
        out 0xe9, al
        "#, in("al") b);
    }
}

pub fn print(args: core::fmt::Arguments) {
    if let Some(s) = args.as_str() {
        for b in s.bytes() {
            write_byte(b);
        }
    } else {
        struct Writer;
        impl core::fmt::Write for Writer {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                for b in s.bytes() {
                    write_byte(b);
                }
                Ok(())
            }
        }
        core::fmt::write(&mut Writer, args).unwrap();
    }
}
