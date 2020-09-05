use iftem_core::error::AgentError;
use std::{error, fmt};

#[derive(Debug)]
pub enum DBError {
    DieselError(diesel::result::Error),
    SensorNotFound(i32),
}

impl fmt::Display for DBError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DBError::SensorNotFound(id) => write!(f, "Did not found sensor: {}", id),
            DBError::DieselError(e) => e.fmt(f),
        }
    }
}

impl error::Error for DBError {}

impl From<diesel::result::Error> for DBError {
    fn from(err: diesel::result::Error) -> Self {
        DBError::DieselError(err)
    }
}

#[derive(Debug)]
pub enum MQTTError {
    NoSensor(),
    PathError(std::string::String),
    PayloadError(std::string::String),
    ParseError(serde_json::error::Error),
    SendError(mqtt_async_client::Error),
}

impl fmt::Display for MQTTError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            MQTTError::NoSensor() => write!(f, "No sensor found"),
            MQTTError::PathError(msg) => write!(f, "Patherror: {}", msg),
            MQTTError::PayloadError(msg) => write!(f, "Invalid payload: {}", msg),
            MQTTError::ParseError(e) => e.fmt(f),
            MQTTError::SendError(e) => e.fmt(f),
        }
    }
}

impl error::Error for MQTTError {}

impl From<mqtt_async_client::Error> for MQTTError {
    fn from(err: mqtt_async_client::Error) -> Self {
        MQTTError::SendError(err)
    }
}

impl From<serde_json::error::Error> for MQTTError {
    fn from(err: serde_json::error::Error) -> Self {
        MQTTError::ParseError(err)
    }
}

#[derive(Debug)]
pub enum PluginError {
    CompilerMismatch(std::string::String, std::string::String),
    Duplicate(std::string::String),
    LibError(libloading::Error),
    AgentStateError(iftem_core::AgentState),
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PluginError::CompilerMismatch(expected, actual) => {
                write!(f, "Compiler needed {}, was {}", expected, actual)
            }
            PluginError::Duplicate(uuid) => write!(f, "Duplicate {}", uuid),
            PluginError::LibError(e) => e.fmt(f),
            PluginError::AgentStateError(state) => write!(f, "Invalid agent state: {:?}", state),
        }
    }
}

impl error::Error for PluginError {}

impl From<libloading::Error> for PluginError {
    fn from(err: libloading::Error) -> Self {
        PluginError::LibError(err)
    }
}

#[derive(Debug)]
pub enum ObserverError {
    User(Box<dyn error::Error>),
    Internal(Box<dyn error::Error>),
}

impl fmt::Display for ObserverError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ObserverError::User(err) => err.fmt(f),
            ObserverError::Internal(err) => err.fmt(f),
        }
    }
}

impl error::Error for ObserverError {}

impl From<DBError> for ObserverError {
    fn from(err: DBError) -> Self {
        match err {
            DBError::SensorNotFound(_) => ObserverError::User(Box::from(err)),
            DBError::DieselError(_) => ObserverError::Internal(Box::from(err)),
        }
    }
}

impl From<AgentError> for ObserverError {
    fn from(err: AgentError) -> Self {
        ObserverError::User(Box::from(err))
    }
}

impl From<MQTTError> for ObserverError {
    fn from(err: MQTTError) -> Self {
        ObserverError::Internal(Box::from(err))
    }
}

impl From<PluginError> for ObserverError {
    fn from(err: PluginError) -> Self {
        ObserverError::User(Box::from(err))
    }
}
