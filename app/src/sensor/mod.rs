pub mod handle;
pub mod observer;

mod container;
#[cfg(test)]
mod test;

pub use observer::agent::AgentObserver;
pub use observer::ConcurrentObserver;

pub enum SensorMessage {
    Data(tracing::Span, ripe_core::SensorDataMessage),
    Log(tracing::Span, std::string::String),
    Reconnected(tracing::Span),
}
