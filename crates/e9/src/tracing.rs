extern crate alloc;

use alloc::format;

use tracing::field::{Field, Visit};
use tracing::Subscriber;
use tracing_subscriber::layer::{self, Context};

pub struct Layer;

impl<S: Subscriber> layer::Layer<S> for Layer {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>, _ctx: Context<'_, S>) -> bool {
        true
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
        let level = meta.level();
        let origin = meta
            .file()
            .and_then(|file| meta.line().map(|ln| format!("{}:{}", file, ln)))
            .unwrap_or_default();

        crate::print!("{level} {origin}");

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
