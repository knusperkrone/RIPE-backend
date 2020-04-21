use plugins_core::Payload;
use serde::{Serialize, Serializer};

#[cfg(test)]
pub mod mock;
pub mod plugin;

pub use plugin::Agent;

pub enum AgentPayload {
    Bool(bool),
    Int(i32),
}

impl Serialize for AgentPayload {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            AgentPayload::Bool(b) => serializer.serialize_bool(*b),
            AgentPayload::Int(i) => serializer.serialize_i32(*i),
        }
    }
}

impl From<Payload> for AgentPayload {
    fn from(other: Payload) -> Self {
        match other {
            Payload::Bool(b) => AgentPayload::Bool(b),
            Payload::Int(i) => AgentPayload::Int(i),
        }
    }
}
