use super::{Agent, AgentFactoryTrait};
use crate::error::PluginError;
use crate::logging::APP_LOGGING;
use crate::sensor::handle::SensorMQTTCommand;
use iftem_core::{error::AgentError, AgentMessage, AgentTrait, PluginDeclaration};
use libloading::Library;
use std::{collections::HashMap, fmt::Debug, sync::Arc};
use tokio::sync::mpsc::{channel, Sender, UnboundedSender};

#[derive(Debug)]
pub struct NativeAgentFactory {
    agent_sender: UnboundedSender<SensorMQTTCommand>,
    libraries: HashMap<String, Arc<Library>>,
}

impl AgentFactoryTrait for NativeAgentFactory {
    fn create_agent(
        &self,
        sensor_id: i32,
        agent_name: &str,
        domain: &str,
        state_json: Option<&str>,
    ) -> Result<Agent, AgentError> {
        let (plugin_sender, plugin_receiver) = channel::<AgentMessage>(64);
        let native_agent =
            unsafe { self.build_native_agent(agent_name, state_json, plugin_sender) };

        if let Some(plugin_agent) = native_agent {
            Ok(Agent::new(
                self.agent_sender.clone(),
                plugin_receiver,
                sensor_id,
                domain.clone().to_owned(),
                agent_name.clone().to_owned(),
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

        let ext = ext_res.unwrap();
        if (cfg!(unix) && ext == "so") || (cfg!(windows) && ext == "dll") {
            let filename = path.to_str().unwrap();
            let res = unsafe { self.load_native_library(filename) };
            return match res {
                Ok((lib_name, _version)) => {
                    info!(APP_LOGGING, "Loaded NATIVE plugin {}", filename);
                    Some(lib_name.to_owned())
                }
                Err(err) => {
                    warn!(APP_LOGGING, "Invalid NATIVE plugin {}: {}", filename, err);
                    None
                }
            };
        }
        None
    }
}

impl NativeAgentFactory {
    pub fn new(sender: UnboundedSender<SensorMQTTCommand>) -> Self {
        NativeAgentFactory {
            agent_sender: sender,
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

        if decl.rustc_version != iftem_core::RUSTC_VERSION
            || decl.core_version != iftem_core::CORE_VERSION
        {
            // version checks to prevent accidental ABI incompatibilities
            return Err(PluginError::CompilerMismatch(
                decl.rustc_version.to_owned(),
                iftem_core::RUSTC_VERSION.to_owned(),
            ));
        } else if let Some(loaded_lib) = self.libraries.get(decl.agent_name) {
            // Check duplicated or outdated library
            let loaded_decl = loaded_lib
                .get::<*mut PluginDeclaration>(b"plugin_declaration\0")?
                .read();

            if loaded_decl.agent_version <= decl.agent_version {
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
    ) -> Option<Box<dyn AgentTrait>> {
        if let Some(library) = self.libraries.get(agent_name) {
            let decl = library
                .get::<*mut PluginDeclaration>(b"plugin_declaration\0")
                .unwrap()
                .read(); // Panic is impossible

            let logger: &slog::Logger = once_cell::sync::Lazy::force(&APP_LOGGING);
            let proxy = (decl.agent_builder)(state_json.to_owned(), logger.clone(), plugin_sender);
            Some(proxy)
        } else {
            None
        }
    }
}
