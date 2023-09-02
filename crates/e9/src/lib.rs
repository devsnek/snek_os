#![no_std]

#[cfg(feature = "tracing")]
pub mod tracing;

fn write_byte(b: u8) {
    if b == b'\n' {
        write_byte(b'\r');
    }
    unsafe {
        core::arch::asm!(r#"
        out 0e9h, al
        "#, in("al") b);
    }
}

#[doc(hidden)]
pub fn _print(args: core::fmt::Arguments) {
    let mut b: u8;
    unsafe {
        core::arch::asm!(r#"
        in al, 0e9h
        "#, out("al") b);
    }
    if b != 0xe9 {
        return;
    }

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

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[macro_export]
macro_rules! dbg {
    () => {
        $crate::println!("[{}:{}]", file!(), line!());
    };
    ($val:expr) => {
        match $val {
            tmp => {
                $crate::println!("[{}:{}] {} = {:#?}", file!(), line!(), stringify!($val), &tmp);
                tmp
            }
        }
    };
    ($val:expr,) => { $crate::dbg!($val) };
    ($($val:expr),+ $(,)?) => {
        ($($crate::dbg!($val)),+,)
    };
}
