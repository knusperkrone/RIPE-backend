pub mod handle;
pub mod observer;

mod container;
#[cfg(test)]
mod test;

pub use observer::agent::AgentObserver;
pub use observer::ConcurrentObserver;

pub enum SensorMessage {
    Data(ripe_core::SensorDataMessage),
    Log(std::string::String),
    Reconnect,
}
