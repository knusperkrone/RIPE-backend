use iftem_core::error::AgentError;
use std::error;
use std::fmt;

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
    SendError(tokio::sync::mpsc::error::SendError<rumq_client::Request>),
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

impl From<tokio::sync::mpsc::error::SendError<rumq_client::Request>> for MQTTError {
    fn from(err: tokio::sync::mpsc::error::SendError<rumq_client::Request>) -> Self {
        MQTTError::SendError(err)
    }
}

impl From<serde_json::error::Error> for MQTTError {
    fn from(err: serde_json::error::Error) -> Self {
        MQTTError::ParseError(err)
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
