use std::fmt;
use std::error;

#[derive(Debug)]
pub enum AgentError {
    InvalidConfig(std::string::String),
    InvalidDomain(std::string::String),
    InvalidIdentifier(std::string::String),
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AgentError::InvalidConfig(msg) => write!(f, "invalid config: {}", msg),
            AgentError::InvalidDomain(msg) => write!(f, "Invalid domain: {}", msg),
            AgentError::InvalidIdentifier(msg) => write!(f, "Invalid identifier: {}", msg),
        }
    }
}

impl error::Error for AgentError {}
