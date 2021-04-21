pub mod agent;
pub mod test;

mod native;
mod wasm;

use iftem_core::error::AgentError;
use std::path::Path;
use test::MockAgent;

pub use agent::Agent;
pub(crate) use native::NativeAgentFactory;
pub(crate) use wasm::WasmAgentFactory;

use crate::sensor::handle::SensorMQTTCommand;
use iftem_core::AgentMessage;
use tokio::sync::mpsc::{channel, UnboundedSender};

pub trait AgentFactoryTrait {
    fn create_agent(
        &self,
        sensor_id: i32,
        agent_name: &str,
        domain: &str,
        state_json: Option<&str>,
    ) -> Result<Agent, iftem_core::error::AgentError>;
    fn agents(&self) -> Vec<String>;
    fn load_plugin_file(&mut self, path: &std::path::PathBuf) -> Option<String>;
}

pub struct AgentFactory {
    sender: UnboundedSender<SensorMQTTCommand>,
    wasm_factory: WasmAgentFactory,
    native_factory: NativeAgentFactory,
}

impl AgentFactory {
    pub fn new(sender: UnboundedSender<SensorMQTTCommand>) -> Self {
        AgentFactory {
            sender: sender.clone(),
            wasm_factory: WasmAgentFactory::new(sender.clone()),
            native_factory: NativeAgentFactory::new(sender.clone()),
        }
    }

    pub fn load_new_plugins(&mut self, dir: &Path) -> Vec<String> {
        // Read files and filter all "basename.extension"
        let entries_res = std::fs::read_dir(dir);
        if entries_res.is_err() {
            error!(crate::logging::APP_LOGGING, "Invalid plugin dir: {:?}", dir);
        }

        let entries = entries_res
            .unwrap()
            .into_iter()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path: std::path::PathBuf = entry.path();
                if path.is_file() && path.extension().is_some() {
                    Some(path)
                } else {
                    None
                }
            });

        // TODO: Sensor reload
        for entry in entries {
            self.native_factory.load_plugin_file(&entry);
            self.wasm_factory.load_plugin_file(&entry);
        }
        Vec::new()
    }
}

impl AgentFactoryTrait for AgentFactory {
    fn create_agent(
        &self,
        sensor_id: i32,
        agent_name: &str,
        domain: &str,
        state_json: Option<&str>,
    ) -> Result<Agent, iftem_core::error::AgentError> {
        if cfg!(test) && agent_name == "MockAgent" {
            let (_, plugin_receiver) = channel::<AgentMessage>(64);
            Ok(Agent::new(
                self.sender.clone(),
                plugin_receiver,
                sensor_id,
                domain.to_owned(),
                agent_name.to_owned(),
                Box::new(MockAgent::new()),
            ))
        } else if self.wasm_factory.has_agent(agent_name) {
            self.wasm_factory
                .create_agent(sensor_id, agent_name, domain, state_json)
        } else if self.native_factory.has_agent(agent_name) {
            self.native_factory
                .create_agent(sensor_id, agent_name, domain, state_json)
        } else {
            Err(AgentError::InvalidIdentifier(format!(
                "Agent not found {}",
                agent_name
            )))
        }
    }

    fn agents(&self) -> Vec<String> {
        let mut agents = Vec::new();
        agents.append(&mut self.native_factory.agents());
        agents.append(&mut self.wasm_factory.agents());

        agents
    }

    fn load_plugin_file(&mut self, _path: &std::path::PathBuf) -> Option<String> {
        None
    }
}
