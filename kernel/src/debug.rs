#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::arch::_print(format_args!($($arg)*)));
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
