//! Event types and async event stream.
//!
//! Provides [`TuiEventStream`] which multiplexes terminal events
//! with timer ticks via crossterm's async event stream.

pub mod broker;
pub mod stream;

pub use broker::EventBroker;
pub use stream::TuiEventStream;
