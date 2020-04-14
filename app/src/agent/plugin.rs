use crate::agent::AgentRegisterConfig;
use crate::logging::APP_LOGGING;
use crate::models::AgentConfigDao;
use crate::models::NewAgentConfig;
use libloading::Library;
use plugins_core::{error::AgentError, AgentTrait, Payload, PluginDeclaration, SensorData};
use std::{collections::HashMap, ffi::OsStr, io, string::String};

pub struct Agent {
    proxy: Box<dyn AgentTrait>,
}

impl Agent {
    fn new(proxy: Box<dyn AgentTrait>) -> Self {
        Agent { proxy: proxy }
    }

    pub fn on_data(&mut self, data: &SensorData) -> Option<Payload> {
        self.proxy.on_data(data)
    }

    pub fn deserialize(&self) -> NewAgentConfig {
        self.proxy.deserialize().into()
    }
}

#[derive(Default)]
pub struct AgentFactory {
    libraries: HashMap<String, Library>,
}

impl AgentFactory {
    pub fn new() -> Self {
        let mut factory = AgentFactory {
            libraries: HashMap::new(),
        };

        let plugins_dir = std::path::Path::new("./plugins/target/release/");
        for entry in std::fs::read_dir(plugins_dir).unwrap() {
            let path: std::path::PathBuf = entry.unwrap().path();
            if path.is_file() && path.extension().is_some() {
                let ext = path.extension().unwrap();
                // load dynamic libraries
                if (cfg!(unix) && ext == "so") || (cfg!(windows) && ext == "dll") {
                    unsafe {
                        match factory.load_library(path.as_os_str()) {
                            Ok(_) => info!(APP_LOGGING, "Loaded plugin {:?}", path),
                            Err(err) => {
                                error!(APP_LOGGING, "Failed Loaded plugin {:?} - {}", path, err)
                            }
                        }
                    }
                }
            }
        }

        factory
    }

    pub fn new_agent(&self, config: &AgentRegisterConfig) -> Result<Agent, AgentError> {
        unsafe { Ok(self.build_agent(&config.agent_name, None)?) }
    }

    pub fn restore_agent(&self, config: &AgentConfigDao) -> Result<Agent, AgentError> {
        unsafe { Ok(self.build_agent(&config.agent_impl, Some(&config.state_json))?) }
    }

    unsafe fn load_library<P: AsRef<OsStr>>(&mut self, library_path: P) -> io::Result<()> {
        // load the library into memory
        let library = Library::new(library_path)?;

        // get a pointer to the plugin_declaration symbol.
        let decl = library
            .get::<*mut PluginDeclaration>(b"plugin_declaration\0")?
            .read();

        
        if decl.rustc_version != plugins_core::RUSTC_VERSION
            || decl.core_version != plugins_core::CORE_VERSION
        {
            // version checks to prevent accidental ABI incompatibilities
            Err(io::Error::new(io::ErrorKind::Other, "Version mismatch"))
        } else if self.libraries.contains_key(decl.agent_name) {
            
            Err(io::Error::new(io::ErrorKind::Other, "Duplicate library"))
        } else {
            self.libraries.insert(decl.agent_name.to_owned(), library);
            Ok(())
        }
    }

    unsafe fn build_agent(
        &self,
        agent_name: &String,
        state_json: Option<&String>,
    ) -> Result<Agent, AgentError> {
        if let Some(library) = self.libraries.get(agent_name) {
            let decl = library
                .get::<*mut PluginDeclaration>(b"plugin_declaration\0")
                .unwrap() // Panic should be impossible
                .read();
            let plugin_agent = (decl.agent_builder)(state_json);
            Ok(Agent::new(plugin_agent))
        } else {
            Err(AgentError::InvalidIdentifier(agent_name.clone()))
        }
    }
}
