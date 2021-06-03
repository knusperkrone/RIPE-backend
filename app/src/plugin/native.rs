use super::AgentLib;
use super::{Agent, AgentFactoryTrait};
use crate::error::PluginError;
use crate::logging::APP_LOGGING;
use crate::sensor::handle::SensorMQTTCommand;
use libloading::Library;
use ripe_core::{error::AgentError, AgentMessage, AgentTrait, PluginDeclaration};
use std::sync::Arc;
use std::{collections::HashMap, fmt::Debug};
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};

#[derive(Debug)]
pub struct NativeAgentFactory {
    iac_sender: UnboundedSender<SensorMQTTCommand>,
    libraries: HashMap<String, Arc<Library>>,
}

impl AgentFactoryTrait for NativeAgentFactory {
    fn create_agent(
        &self,
        sensor_id: i32,
        agent_name: &str,
        domain: &str,
        state_json: Option<&str>,
        plugin_sender: Sender<AgentMessage>,
        plugin_receiver: Receiver<AgentMessage>,
    ) -> Result<Agent, AgentError> {
        // UNSAFE: Build agent from checked native lib
        let native_agent =
            unsafe { self.build_native_agent(agent_name, state_json, plugin_sender) };

        if let Some((plugin_agent, lib)) = native_agent {
            Ok(Agent::new(
                self.iac_sender.clone(),
                plugin_receiver,
                sensor_id,
                domain.to_owned(),
                AgentLib(lib),
                agent_name.to_owned(),
                plugin_agent,
            ))
        } else {
            Err(AgentError::InvalidIdentifier(agent_name.to_owned()))
        }
    }

    fn agents(&self) -> Vec<String> {
        self.libraries.keys().map(|name| name.clone()).collect()
    }

    fn load_plugin_file(&mut self, path: &std::path::PathBuf) -> Option<String> {
        let ext_res = path.extension();
        if ext_res.is_none() {
            return None;
        }

        let ext = ext_res.unwrap().to_str().unwrap_or_default();
        if (cfg!(unix) && ext.starts_with("so")) || (cfg!(windows) && ext.starts_with("dll")) {
            let filename = path.to_str().unwrap();
            // UNSAFE: load and check native lib
            let res = unsafe { self.load_native_library(filename) };
            return match res {
                Ok((lib_name, _version)) => {
                    info!(APP_LOGGING, "Loaded native: {}", filename);
                    Some(lib_name.to_owned())
                }
                Err(err) => {
                    warn!(APP_LOGGING, "Invalid native {}: {}", filename, err);
                    None
                }
            };
        }
        None
    }
}

impl NativeAgentFactory {
    pub fn new(iac_sender: UnboundedSender<SensorMQTTCommand>) -> Self {
        NativeAgentFactory {
            iac_sender,
            libraries: HashMap::new(),
        }
    }

    pub fn has_agent(&self, agent_name: &str) -> bool {
        self.libraries.contains_key(agent_name)
    }

    unsafe fn load_native_library(
        &mut self,
        library_path: &str,
    ) -> Result<(String, u32), PluginError> {
        // load the library into memory
        let library = Library::new(library_path)?;

        // get a pointer to the plugin_declaration symbol.
        let decl = library
            .get::<*mut PluginDeclaration>(b"plugin_declaration\0")?
            .read();

        if decl.rustc_version != ripe_core::RUSTC_VERSION
            || decl.core_version != ripe_core::CORE_VERSION
        {
            // version checks to prevent accidental ABI incompatibilities
            return Err(PluginError::CompilerMismatch(
                decl.rustc_version.to_owned(),
                ripe_core::RUSTC_VERSION.to_owned(),
            ));
        } else if let Some(current_lib) = self.libraries.get(decl.agent_name) {
            // Check duplicated or outdated library
            let current_decl = current_lib
                .get::<*mut PluginDeclaration>(b"plugin_declaration\0")?
                .read();

            if cfg!(prod) && current_decl.agent_version <= decl.agent_version {
                return Err(PluginError::Duplicate(
                    decl.agent_name.to_owned(),
                    decl.agent_version,
                ));
            }
        }

        self.libraries
            .insert(decl.agent_name.to_owned(), Arc::new(library));
        return Ok((decl.agent_name.to_owned(), decl.agent_version));
    }

    unsafe fn build_native_agent(
        &self,
        agent_name: &str,
        state_json: Option<&str>,
        plugin_sender: Sender<AgentMessage>,
    ) -> Option<(Box<dyn AgentTrait>, Arc<Library>)> {
        if let Some(library) = self.libraries.get(agent_name) {
            let decl = library
                .get::<*mut PluginDeclaration>(b"plugin_declaration\0")
                .unwrap()
                .read(); // Panic is impossible

            let logger = APP_LOGGING.clone();
            let proxy = (decl.agent_builder)(state_json, logger, plugin_sender);
            Some((proxy, library.clone()))
        } else {
            None
        }
    }
}
