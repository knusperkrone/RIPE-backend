pub mod agent;
pub mod test;

mod native;
mod wasm;

use iftem_core::error::AgentError;
use std::{collections::HashMap, ffi::OsString, path::Path, time::SystemTime};
use test::MockAgent;

pub use agent::Agent;
pub(crate) use native::NativeAgentFactory;
pub(crate) use wasm::WasmAgentFactory;

use crate::{logging::APP_LOGGING, sensor::handle::SensorMQTTCommand};
use iftem_core::AgentMessage;
use tokio::sync::mpsc::{channel, Receiver, Sender, UnboundedSender};

pub trait AgentFactoryTrait {
    type Lib;

    fn create_agent(
        &self,
        sensor_id: i32,
        agent_name: &str,
        domain: &str,
        state_json: Option<&str>,
        plugin_sender: Sender<AgentMessage>,
        plugin_receiver: Receiver<AgentMessage>,
    ) -> Result<Agent, iftem_core::error::AgentError>;
    fn agents(&self) -> Vec<String>;
    fn load_plugin_file(
        &mut self,
        path: &std::path::PathBuf,
    ) -> Option<(String, Option<Self::Lib>)>;
}

pub struct AgentFactory {
    sender: UnboundedSender<SensorMQTTCommand>,
    wasm_factory: WasmAgentFactory,
    native_factory: NativeAgentFactory,
    timestamps: HashMap<OsString, SystemTime>,
}

unsafe impl Send for AgentFactory {}
unsafe impl Sync for AgentFactory {}

impl AgentFactory {
    pub fn new(iac_sender: UnboundedSender<SensorMQTTCommand>) -> Self {
        AgentFactory {
            sender: iac_sender.clone(),
            wasm_factory: WasmAgentFactory::new(iac_sender.clone()),
            native_factory: NativeAgentFactory::new(iac_sender.clone()),
            timestamps: HashMap::new(),
        }
    }

    pub fn load_new_plugins(&mut self, dir: &Path) -> Vec<(String, Box<dyn std::any::Any>)> {
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
                let modified = entry.metadata().ok()?.created().ok()?;
                return Some((entry, modified));
            })
            .filter_map(|(entry, modified)| {
                let path: std::path::PathBuf = entry.path();
                if path.extension().is_some() {
                    Some((path, modified))
                } else {
                    None
                }
            });

        let mut ret: Vec<(String, Box<dyn std::any::Any>)> = Vec::new();
        for (path, modified_time) in entries {
            if let Some(old_time) = self.timestamps.get(path.as_os_str()) {
                if old_time == &modified_time {
                    debug!(APP_LOGGING, "Skipped updating {:?}", path);
                    continue;
                }
            }
            self.timestamps
                .insert(path.as_os_str().to_owned(), modified_time);

            if let Some((lib_name, old_lib)) = self.native_factory.load_plugin_file(&path) {
                ret.push((lib_name, Box::new(old_lib)));
            }
            if let Some((lib_name, old_lib)) = self.wasm_factory.load_plugin_file(&path) {
                ret.push((lib_name, Box::new(old_lib)));
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
    ) -> Result<Agent, iftem_core::error::AgentError> {
        let (plugin_sender, plugin_receiver) = channel::<AgentMessage>(256);
        if cfg!(test) && agent_name == "MockAgent" {
            Ok(Agent::new(
                self.sender.clone(),
                plugin_receiver,
                sensor_id,
                domain.to_owned(),
                agent_name.to_owned(),
                Box::new(MockAgent::new(plugin_sender)),
            ))
        } else if self.wasm_factory.has_agent(agent_name) {
            self.wasm_factory.create_agent(
                sensor_id,
                agent_name,
                domain,
                state_json,
                plugin_sender,
                plugin_receiver,
            )
        } else if self.native_factory.has_agent(agent_name) {
            self.native_factory.create_agent(
                sensor_id,
                agent_name,
                domain,
                state_json,
                plugin_sender,
                plugin_receiver,
            )
        } else {
            Err(AgentError::InvalidIdentifier(format!(
                "Agent not found {}",
                agent_name
            )))
        }
    }

    pub fn agents(&self) -> Vec<String> {
        let mut agents = Vec::new();
        agents.append(&mut self.native_factory.agents());
        agents.append(&mut self.wasm_factory.agents());

        agents
    }
}
