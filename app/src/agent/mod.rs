use plugins_core::Payload;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::string::String;

#[cfg(test)]
pub mod mock;
pub mod plugin;

pub use plugin::Agent;

// serializable variant
#[derive(Deserialize, Serialize)]
pub enum AgentPayload {
    Bool(bool),
    Int(i32),
}

impl From<Payload> for AgentPayload { 
    fn from(other: Payload) -> Self {
        match other {
            Payload::Bool(b) => AgentPayload::Bool(b),
            Payload::Int(i) => AgentPayload::Int(i),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AgentRegisterConfig {
    pub domain: String,
    pub agent_name: String,
}
