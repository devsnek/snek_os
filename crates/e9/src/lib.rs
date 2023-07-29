#![no_std]

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {{
        fn write_byte(b: u8) {
            unsafe {
                core::arch::asm!(r#"
                mov al, {}
                out 0e9h, al
                "#, in(reg_byte) b);
            }
        }
        match format_args!($($arg)*) {
            args => {
                if let Some(s) = args.as_str() {
                    for b in s.bytes() {
                        write_byte(b);
                    }
                } else {
                    struct Foo;
                    impl core::fmt::Write for Foo {
                        fn write_str(&mut self, s: &str) -> core::fmt::Result {
                            for b in s.bytes() {
                                write_byte(b);
                            }
                            Ok(())
                        }
                    }
                    core::fmt::write(&mut Foo, args).unwrap();
                }
                write_byte(b'\r');
                write_byte(b'\n');
            }
        }
    }}
}

#[macro_export]
macro_rules! dbg {
    () => {
        println!("[{}:{}]", core::file!(), core::line!())
    };
    ($val:expr $(,)?) => {
        match $val {
            tmp => {
                println!("[{}:{}] {} = {:#?}",
                    core::file!(), core::line!(), core::stringify!($val), &tmp);
                tmp
            }
        }
    };
    ($($val:expr),+ $(,)?) => {
        ($(dbg!($val)),+,)
    };
}
