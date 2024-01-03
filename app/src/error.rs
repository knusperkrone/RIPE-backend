use ripe_core::error::AgentError;
use std::error;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DBError {
    #[error(transparent)]
    SQLError(#[from] sqlx::Error),
    #[error("Did not found sensor: {0}")]
    SensorNotFound(i32),
}

#[derive(Debug, Error)]
pub enum MQTTError {
    #[error("Invalid Path: {0}")]
    Path(std::string::String),
    #[error("Invalid Payload: {0}")]
    Payload(std::string::String),
    #[error("Invalid JSON: {0}")]
    Parse(#[from] serde_json::error::Error),
    #[error("Send Failed: {0}")]
    Send(#[from] paho_mqtt::Error),
    #[error("Timeout")]
    Timeout(),
    #[error("Failed acquiring read lock")]
    ReadLock(),
    #[error("Failed acquiring write lock")]
    WriteLock(),
}

#[derive(Debug, Clone, Error)]
pub enum WasmPluginError {
    #[error("Plugin already loaded")]
    Duplicate,
    #[error("Failed calling method")]
    CallError,
    #[error("Compliling the module failed: {0}")]
    CompileError(std::string::String),
    #[error("Plugin contract not fullfilled: {0}")]
    ContractMismatch(std::string::String),
}

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
            wasmer::InstantiationError::Link(e) => WasmPluginError::ContractMismatch(e.to_string()),
            wasmer::InstantiationError::Start(e) => {
                WasmPluginError::ContractMismatch(e.to_string())
            }
            wasmer::InstantiationError::CpuFeature(e) => {
                WasmPluginError::ContractMismatch(e.to_string())
            }
            wasmer::InstantiationError::DifferentStores => {
                WasmPluginError::ContractMismatch("Different Stores".to_string())
            }
            wasmer::InstantiationError::DifferentArchOS => {
                WasmPluginError::ContractMismatch("Different Arch".to_string())
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("Plugin was compliled with {1}, but needed > {0}")]
    CompilerMismatch(std::string::String, std::string::String),
    #[error("Duplicate {0}, version = {1}")]
    Duplicate(std::string::String, u32),
    #[error(transparent)]
    LibError(#[from] libloading::Error),
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("Arguments are not used as specified")]
    ArgumentError(),
}

#[derive(Debug, Error)]
#[error(transparent)]
pub enum ObserverError {
    User(Box<dyn error::Error>),
    Internal(Box<dyn error::Error>),
}
unsafe impl Send for ObserverError {}

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

impl From<ApiError> for ObserverError {
    fn from(err: ApiError) -> Self {
        ObserverError::User(Box::from(err))
    }
}
