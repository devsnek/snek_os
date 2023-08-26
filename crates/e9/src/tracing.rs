use tracing::field::{Field, Visit};
use tracing::{Level, Subscriber};
use tracing_subscriber::layer::{self, Context};

pub struct Layer {
    level: Level,
}

impl Layer {
    pub fn new(level: Level) -> Self {
        Self { level }
    }
}

impl<S: Subscriber> layer::Layer<S> for Layer {
    fn enabled(&self, meta: &tracing::Metadata<'_>, _ctx: Context<'_, S>) -> bool {
        meta.level() <= &self.level
    }

    fn on_new_span(
        &self,
        _attrs: &tracing::span::Attributes<'_>,
        _id: &tracing::Id,
        _ctx: Context<'_, S>,
    ) {
    }

    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let target = meta.target();
        let level = meta.level();

        static LOCK: spin::Mutex<()> = spin::Mutex::new(());
        let _guard = LOCK.lock();

        crate::print!("{level} {target}");

        struct Visitor;
        impl Visit for Visitor {
            fn record_debug(&mut self, field: &Field, value: &dyn core::fmt::Debug) {
                crate::print!("; {} = {:?}", field.name(), value);
            }
        }
        event.record(&mut Visitor);
        crate::println!();
    }

    fn on_record(
        &self,
        _id: &tracing::Id,
        _values: &tracing::span::Record<'_>,
        _ctx: Context<'_, S>,
    ) {
    }

    fn on_enter(&self, _id: &tracing::Id, _ctx: Context<'_, S>) {}

    fn on_exit(&self, _id: &tracing::Id, _ctx: Context<'_, S>) {}
}
