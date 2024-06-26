pub mod agent;
pub mod test;

mod native;
mod wasm;

use ripe_core::{error::AgentError, AgentStream};
use std::{collections::HashMap, ffi::OsString, path::Path, time::SystemTime};
use test::MockAgent;

pub use agent::Agent;
pub(crate) use native::NativeAgentFactory;
pub(crate) use wasm::WasmAgentFactory;

use crate::sensor::handle::SensorMQTTCommand;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, warn};

#[derive(Clone)]
pub struct AgentLib(std::sync::Arc<dyn std::any::Any>);
unsafe impl Send for AgentLib {}
unsafe impl Sync for AgentLib {}

pub trait AgentFactoryTrait {
    fn create_agent(
        &self,
        sensor_id: i32,
        agent_name: &str,
        domain: &str,
        state_json: Option<&str>,
        stream: AgentStream,
    ) -> Result<Agent, ripe_core::error::AgentError>;
    fn agents(&self) -> Vec<&String>;
    fn load_plugin_file(&mut self, path: &Path) -> Option<String>;
}

#[derive(Debug)]
pub struct AgentFactory {
    sender: UnboundedSender<SensorMQTTCommand>,
    wasm_factory: WasmAgentFactory,
    native_factory: NativeAgentFactory,
    timestamps: HashMap<OsString, SystemTime>,
}

impl AgentFactory {
    pub fn new(iac_sender: UnboundedSender<SensorMQTTCommand>) -> Self {
        AgentFactory {
            sender: iac_sender.clone(),
            wasm_factory: WasmAgentFactory::new(iac_sender.clone()),
            native_factory: NativeAgentFactory::new(iac_sender.clone()),
            timestamps: HashMap::new(),
        }
    }

    pub fn load_new_plugins(&mut self, dir: &Path) -> Vec<String> {
        // Read files and filter all "basename.extension"
        let entries_res = std::fs::read_dir(dir);
        if entries_res.is_err() {
            error!("Invalid plugin dir: {:?}", dir);
        }

        let entries = entries_res
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| {
                if let Ok(metadata) = entry.metadata() {
                    let mut modified = SystemTime::UNIX_EPOCH;
                    if let Ok(created) = metadata.created() {
                        modified = created;
                    } else {
                        warn!("No created metadata for {:?}", entry);
                    }
                    (entry, modified)
                } else {
                    warn!("No metadata for {:?}", entry);
                    (entry, SystemTime::UNIX_EPOCH)
                }
            })
            .filter_map(|(entry, modified)| {
                let path: std::path::PathBuf = entry.path();
                if path.extension().is_some() {
                    Some((path, modified))
                } else {
                    None
                }
            });

        let mut ret: Vec<String> = Vec::new();
        for (path, modified_time) in entries {
            if let Some(old_time) = self.timestamps.get(path.as_os_str()) {
                if old_time == &modified_time {
                    continue;
                }
            }
            self.timestamps
                .insert(path.as_os_str().to_owned(), modified_time);

            if let Some(lib_name) = self.native_factory.load_plugin_file(&path) {
                ret.push(lib_name)
            }
            if let Some(lib_name) = self.wasm_factory.load_plugin_file(&path) {
                ret.push(lib_name)
            }
        }
        ret
    }

    pub fn create_agent(
        &self,
        sensor_id: i32,
        agent_name: &str,
        domain: &str,
        state_json: Option<&str>,
    ) -> Result<Agent, ripe_core::error::AgentError> {
        let stream = AgentStream::default();
        if cfg!(test) && agent_name == "MockAgent" {
            Ok(Agent::new(
                self.sender.clone(),
                stream.receiver,
                sensor_id,
                domain.to_owned(),
                AgentLib(std::sync::Arc::new(0)),
                agent_name.to_owned(),
                Box::new(MockAgent::new(stream.sender)),
            ))
        } else if self.wasm_factory.has_agent(agent_name) {
            self.wasm_factory
                .create_agent(sensor_id, agent_name, domain, state_json, stream)
        } else if self.native_factory.has_agent(agent_name) {
            self.native_factory
                .create_agent(sensor_id, agent_name, domain, state_json, stream)
        } else {
            Err(AgentError::InvalidIdentifier(format!(
                "Agent not found {}",
                agent_name
            )))
        }
    }

    pub fn agents(&self) -> Vec<&String> {
        let mut agents = Vec::new();
        agents.append(&mut self.native_factory.agents());
        agents.append(&mut self.wasm_factory.agents());

        agents
    }
}
