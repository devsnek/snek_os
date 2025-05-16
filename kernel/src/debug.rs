use core::sync::atomic::{AtomicPtr, Ordering};
use spin::Mutex;
use tracing::{
    dispatch::set_global_default,
    field::{Field, Visit},
    span::{Attributes, Record},
    Collect, Dispatch, Event, Id, Metadata,
};

static PRINT: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

type PrintFn = fn(core::fmt::Arguments);

fn print(args: core::fmt::Arguments) {
    crate::arch::print(format_args!("{args}"));

    let print = PRINT.load(Ordering::Relaxed);
    if !print.is_null() {
        // SAFETY: value is a non-null pointer
        let print = unsafe { core::mem::transmute::<*mut (), PrintFn>(print) };
        print(args);
    }
}

struct Collector {}

impl Collect for Collector {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(0)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        static LOCK: Mutex<()> = Mutex::new(());
        let _guard = LOCK.lock();

        let meta = event.metadata();
        // let target = meta.target();
        let level = meta.level();

        print(format_args!("{level}"));

        struct Visitor(bool);
        impl Visit for Visitor {
            fn record_debug(&mut self, field: &Field, value: &dyn core::fmt::Debug) {
                if self.0 {
                    self.0 = false;
                    print(format_args!(" "));
                } else {
                    print(format_args!("; "));
                }

                if field.name() == "message" {
                    print(format_args!("{:?}", value));
                } else {
                    print(format_args!("{} = {:?}", field.name(), value));
                    self.0 = false;
                }
            }
        }

        event.record(&mut Visitor(true));

        print(format_args!("\n"));
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}

    fn current_span(&self) -> tracing_core::span::Current {
        tracing_core::span::Current::none()
    }
}

impl log::Log for Collector {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if log::Log::enabled(self, record.metadata()) {
            print(format_args!("{} - {}\n", record.level(), record.args()));
        }
    }

    fn flush(&self) {}
}

static COLLECTOR: Collector = Collector {};

pub fn init() {
    let dispatch = Dispatch::from_static(&COLLECTOR);
    let _ = set_global_default(dispatch);
    let _ = log::set_logger(&COLLECTOR);
    log::set_max_level(log::LevelFilter::Trace);
}

pub fn set_print(f: PrintFn) {
    PRINT.store(f as _, Ordering::Relaxed);
}
