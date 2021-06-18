use ripe_core::error::AgentError;
use std::{error, fmt};

#[derive(Debug)]
pub enum DBError {
    SQLError(sqlx::Error),
    SensorNotFound(i32),
}

impl fmt::Display for DBError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DBError::SensorNotFound(id) => write!(f, "Did not found sensor: {}", id),
            DBError::SQLError(e) => e.fmt(f),
        }
    }
}

impl error::Error for DBError {}

impl From<sqlx::Error> for DBError {
    fn from(err: sqlx::Error) -> Self {
        DBError::SQLError(err)
    }
}

#[derive(Debug)]
pub enum MQTTError {
    PathError(std::string::String),
    PayloadError(std::string::String),
    ParseError(serde_json::error::Error),
    SendError(paho_mqtt::Error),
}

impl fmt::Display for MQTTError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            MQTTError::PathError(msg) => write!(f, "Patherror: {}", msg),
            MQTTError::PayloadError(msg) => write!(f, "Invalid payload: {}", msg),
            MQTTError::ParseError(e) => e.fmt(f),
            MQTTError::SendError(e) => write!(f, "SendError: {}", e),
        }
    }
}

impl error::Error for MQTTError {}

impl From<paho_mqtt::Error> for MQTTError {
    fn from(err: paho_mqtt::Error) -> Self {
        MQTTError::SendError(err)
    }
}

impl From<serde_json::error::Error> for MQTTError {
    fn from(err: serde_json::error::Error) -> Self {
        MQTTError::ParseError(err)
    }
}

#[derive(Debug, Clone)]
pub enum WasmPluginError {
    Duplicate,
    CallError,
    CompileError(std::string::String),
    ContractMismatch(std::string::String),
}

impl fmt::Display for WasmPluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WasmPluginError::Duplicate => {
                write!(f, "Plugin already present")
            }
            WasmPluginError::CallError => {
                write!(f, "Failed calling a method")
            }
            WasmPluginError::CompileError(err) => {
                write!(f, "Compiling the module failed: {}", err)
            }
            WasmPluginError::ContractMismatch(method_name) => {
                write!(f, "Plugin contract not fullfilled: {}", method_name)
            }
        }
    }
}

impl error::Error for WasmPluginError {}

impl From<wasmer::CompileError> for WasmPluginError {
    fn from(err: wasmer::CompileError) -> Self {
        WasmPluginError::CompileError(format!("{:?}", err))
    }
}

impl From<wasmer::ExportError> for WasmPluginError {
    fn from(_: wasmer::ExportError) -> Self {
        WasmPluginError::ContractMismatch("memory".to_owned())
    }
}

impl From<wasmer::RuntimeError> for WasmPluginError {
    fn from(_: wasmer::RuntimeError) -> Self {
        WasmPluginError::CallError
    }
}

impl From<wasmer::InstantiationError> for WasmPluginError {
    fn from(err: wasmer::InstantiationError) -> Self {
        match err {
            wasmer::InstantiationError::Link(e) => {
                WasmPluginError::ContractMismatch(format!("{}", e))
            }
            wasmer::InstantiationError::Start(e) => {
                WasmPluginError::ContractMismatch(format!("{}", e))
            }
            wasmer::InstantiationError::HostEnvInitialization(e) => {
                WasmPluginError::ContractMismatch(format!("{}", e))
            }
        }
    }
}

#[derive(Debug)]
pub enum PluginError {
    CompilerMismatch(std::string::String, std::string::String),
    Duplicate(std::string::String, u32),
    LibError(libloading::Error),
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PluginError::CompilerMismatch(expected, actual) => {
                write!(f, "Compiler needed {}, was {}", expected, actual)
            }
            PluginError::Duplicate(uuid, version) => write!(f, "Duplicate {} v{}", uuid, version),
            PluginError::LibError(e) => e.fmt(f),
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
unsafe impl Send for ObserverError {}

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
            DBError::SQLError(_) => ObserverError::Internal(Box::from(err)),
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
