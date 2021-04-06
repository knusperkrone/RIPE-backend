use crate::error::PluginError;
use crate::logging::APP_LOGGING;
use crate::{models::dao::AgentConfigDao, sensor::handle::SensorMQTTCommand};
use dotenv::dotenv;
use iftem_core::{error::AgentError, AgentMessage, AgentTrait, PluginDeclaration};
use libloading::Library;
use std::{collections::HashMap, env, ffi::OsStr, fmt::Debug, sync::Arc};

use tokio::sync::mpsc::{channel, Sender, UnboundedSender};

use super::Agent;

#[derive(Debug)]
pub struct AgentFactory {
    agent_sender: UnboundedSender<SensorMQTTCommand>,
    libraries: HashMap<String, Arc<Library>>,
}

impl AgentFactory {
    pub fn new(sender: UnboundedSender<SensorMQTTCommand>) -> Self {
        let mut factory = AgentFactory {
            agent_sender: sender,
            libraries: HashMap::new(),
        };
        unsafe {
            factory.load_plugins();
        }
        factory
    }

    pub fn agents(&self) -> Vec<String> {
        self.libraries.keys().map(|name| name.clone()).collect()
    }

    pub fn new_agent(
        &self,
        sensor_id: i32,
        agent_name: &String,
        domain: &String,
    ) -> Result<Agent, AgentError> {
        unsafe { Ok(self.build_agent(sensor_id, agent_name, domain, None)?) }
    }

    pub fn restore_agent(
        &self,
        sensor_id: i32,
        config: &AgentConfigDao,
    ) -> Result<Agent, AgentError> {
        unsafe {
            match self.build_agent(
                sensor_id,
                config.agent_impl(),
                config.domain(),
                Some(config.state_json()),
            ) {
                Ok(agent) => Ok(agent),
                Err(e) => {
                    error!(APP_LOGGING, "Failed restoring agent: {}", e);
                    Err(e)
                }
            }
        }
    }

    pub unsafe fn load_plugins(&mut self) -> Vec<String> {
        dotenv().ok();
        let path = env::var("PLUGIN_DIR").expect("PLUGIN_DIR must be set");
        let plugins_dir = std::path::Path::new(&path);
        let entries_res = std::fs::read_dir(plugins_dir);
        if entries_res.is_err() {
            error!(APP_LOGGING, "Invalid plugin dir: {}", path);
            return vec![];
        }

        entries_res
            .unwrap()
            .into_iter()
            .filter_map(|entry| {
                let path: std::path::PathBuf = entry.unwrap().path();
                if path.is_file() && path.extension().is_some() {
                    // Extact file extension and load .so/.ddl
                    let ext = path.extension().unwrap();
                    if (cfg!(unix) && ext == "so") || (cfg!(windows) && ext == "dll") {
                        match self.load_library(path.as_os_str()) {
                            Ok((lib_name, version)) => {
                                info!(APP_LOGGING, "Loaded plugin {:?} v{}", path, version);
                                return Some(lib_name.to_owned());
                            }
                            Err(err) => {
                                error!(APP_LOGGING, "Failed Loaded plugin {:?} - {}", path, err)
                            }
                        }
                    }
                }
                None
            })
            .collect()
    }

    unsafe fn load_library<P: AsRef<OsStr>>(
        &mut self,
        library_path: P,
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

    pub unsafe fn build_proxy_agent(
        &self,
        agent_name: &String,
        state_json: Option<&String>,
        plugin_sender: &Sender<AgentMessage>,
    ) -> Option<Box<dyn AgentTrait>> {
        if let Some(library) = self.libraries.get(agent_name) {
            let decl = library
                .get::<*mut PluginDeclaration>(b"plugin_declaration\0")
                .unwrap()
                .read(); // Panic should be impossible

            let logger: &slog::Logger = once_cell::sync::Lazy::force(&APP_LOGGING);
            let proxy = (decl.agent_builder)(state_json, logger.clone(), plugin_sender.clone());
            Some(proxy)
        } else {
            None
        }
    }

    unsafe fn build_agent(
        &self,
        sensor_id: i32,
        agent_name: &String,
        domain: &String,
        state_json: Option<&String>,
    ) -> Result<Agent, AgentError> {
        let (plugin_sender, plugin_receiver) = channel::<AgentMessage>(64);
        if cfg!(test) && agent_name == "MockAgent" {
            info!(APP_LOGGING, "Creating mock agent!");
            return Ok(Agent::new(
                self.agent_sender.clone(),
                plugin_sender,
                plugin_receiver,
                sensor_id,
                domain.clone(),
                agent_name.clone(),
                Box::new(crate::plugin::test::MockAgent::new()),
            ));
        }

        if let Some(plugin_agent) = self.build_proxy_agent(agent_name, state_json, &plugin_sender) {
            Ok(Agent::new(
                self.agent_sender.clone(),
                plugin_sender,
                plugin_receiver,
                sensor_id,
                domain.clone(),
                agent_name.clone(),
                plugin_agent,
            ))
        } else {
            Err(AgentError::InvalidIdentifier(agent_name.clone()))
        }
    }
}
