use crate::logging::APP_LOGGING;
use crate::models::dao::AgentConfigDao;
use dotenv::dotenv;
use libloading::Library;
use plugins_core::{error::AgentError, AgentTrait, Payload, PluginDeclaration, SensorData};
use std::env;
use std::{collections::HashMap, ffi::OsStr, io, string::String};

pub struct Agent {
    sensor_id: i32,
    domain: String,
    proxy: Box<dyn AgentTrait>,
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        write!(f, "{:?}", self.proxy)
    }
}

impl Agent {
    pub fn new(sensor_id: i32, domain: String, proxy: Box<dyn AgentTrait>) -> Self {
        Agent {
            sensor_id: sensor_id,
            proxy: proxy,
            domain: domain,
        }
    }

    pub fn on_data(&mut self, data: &SensorData) -> Option<Payload> {
        self.proxy.on_data(data)
    }

    pub fn deserialize(&self) -> AgentConfigDao {
        let config = self.proxy.deserialize();
        AgentConfigDao::new(
            self.sensor_id,
            self.domain.clone(),
            config.name,
            config.state_json,
        )
    }

    pub fn domain(&self) -> &String {
        &self.domain
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
            Ok(self.build_agent(
                sensor_id,
                config.agent_impl(),
                config.domain(),
                Some(config.state_json()),
            )?)
        }
    }

    unsafe fn load_plugins(&mut self) {
        dotenv().ok();
        let plugin_dirs = env::var("PLUGIN_DIRS").expect("PLUGIN_DIRS must be set");
        let dir_list: Vec<&str> = plugin_dirs.split(",").collect();

        for path in dir_list {
            let plugins_dir = std::path::Path::new(path);
            let entries_res = std::fs::read_dir(plugins_dir);
            if entries_res.is_err() {
                error!(APP_LOGGING, "Invalid plugin dir: {}", path);
                continue;
            }

            for entry in entries_res.unwrap() {
                let path: std::path::PathBuf = entry.unwrap().path();
                if path.is_file() && path.extension().is_some() {
                    // Extact file extension and load .so/.ddl
                    let ext = path.extension().unwrap();
                    if (cfg!(unix) && ext == "so") || (cfg!(windows) && ext == "dll") {
                        match self.load_library(path.as_os_str()) {
                            Ok(_) => info!(APP_LOGGING, "Loaded plugin {:?}", path),
                            Err(err) => {
                                error!(APP_LOGGING, "Failed Loaded plugin {:?} - {}", path, err)
                            }
                        }
                    }
                }
            }
        }
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
        sensor_id: i32,
        agent_name: &String,
        domain: &String,
        state_json: Option<&String>,
    ) -> Result<Agent, AgentError> {
        if let Some(library) = self.libraries.get(agent_name) {
            let decl = library
                .get::<*mut PluginDeclaration>(b"plugin_declaration\0")
                .unwrap() // Panic should be impossible
                .read();

            // Init logger and pass it to plugin
            let logger: &slog::Logger = once_cell::sync::Lazy::force(&APP_LOGGING);
            let plugin_agent = (decl.agent_builder)(state_json, logger.clone());
            Ok(Agent::new(sensor_id, domain.clone(), plugin_agent))
        } else {
            Err(AgentError::InvalidIdentifier(agent_name.clone()))
        }
    }
}
