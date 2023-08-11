#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        match format_args!($($arg)*) {
            tmp => {
                e9::_print(tmp);
                $crate::arch::_print(tmp);
            }
        };
    }
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

#[macro_export]
macro_rules! format {
    ($($arg:tt)*) => {{
        let res = alloc::fmt::format(format_args!($($arg)*));
        res
    }}
}

struct Subscriber;

impl tracing::Subscriber for Subscriber {
    fn enabled(&self, _meta: &tracing::Metadata) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::Id {
        tracing::Id::from_u64(0)
    }
    fn record(&self, _: &tracing::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::Id, _: &tracing::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {}
    fn enter(&self, _: &tracing::Id) {}
    fn exit(&self, _: &tracing::Id) {}
}

pub fn init() {
    use tracing_subscriber::layer::Layer;

    tracing::subscriber::set_global_default(e9::tracing::Layer.with_subscriber(Subscriber))
        .unwrap();
}
