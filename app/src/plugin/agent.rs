use crate::error::PluginError;
use crate::logging::APP_LOGGING;
use crate::{
    models::dao::AgentConfigDao,
    rest::{AgentPayload, SensorMessageDto},
};
use dotenv::dotenv;
use futures::future::{AbortHandle, Abortable};
use iftem_core::{
    error::AgentError, AgentMessage, AgentState, AgentTrait, AgentUI, PluginDeclaration,
    SensorDataMessage,
};
use libloading::Library;
use std::{
    collections::HashMap,
    env,
    ffi::OsStr,
    string::String,
    sync::{Arc, RwLock},
};
use tokio::stream::StreamExt;
use tokio::sync::mpsc::{channel, Receiver, Sender};

pub struct Agent {
    sensor_id: i32,
    domain: String,
    plugin_sender: Sender<AgentMessage>,
    agent_name: String,
    abort_handle: AbortHandle,
    agent_proxy: Box<dyn AgentTrait>,
    needs_update: bool,
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        write!(f, "{:?}", self.agent_proxy)
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        self.abort_handle.abort();
    }
}

impl Agent {
    pub fn new(
        agent_sender: Sender<SensorMessageDto>,
        plugin_sender: Sender<AgentMessage>,
        plugin_receiver: Receiver<AgentMessage>,
        sensor_id: i32,
        domain: String,
        agent_name: String,
        agent_proxy: Box<dyn AgentTrait>,
    ) -> Self {
        let state = Arc::new(RwLock::new(AgentState::Default));

        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        let future = Abortable::new(
            Agent::dispatch_plugin_ipc(
                sensor_id,
                domain.clone(),
                state.clone(),
                agent_sender,
                plugin_receiver,
            ),
            abort_registration,
        );
        tokio::spawn(async move { future.await });

        Agent {
            sensor_id,
            domain,
            plugin_sender,
            agent_name,
            abort_handle,
            agent_proxy,
            needs_update: false,
        }
    }

    pub fn reload_agent(&mut self, factory: &AgentFactory) -> Result<(), PluginError> {
        if self.agent_proxy.state() == &AgentState::Active {
            return Err(PluginError::AgentStateError(AgentState::Active));
        }

        let state_json = self.agent_proxy.deserialize().state_json;
        unsafe {
            let proxy = factory
                .build_proxy_agent(&self.agent_name, Some(&state_json), &self.plugin_sender)
                .ok_or_else(|| PluginError::Duplicate(self.agent_name.clone()))?;
            self.agent_proxy = proxy;
        }
        self.needs_update = false;
        Ok(())
    }

    pub fn on_data(&mut self, data: &SensorDataMessage) {
        self.agent_proxy.on_data(data);
    }

    pub fn render_ui(&self, data: &SensorDataMessage) -> AgentUI {
        self.agent_proxy.render_ui(data)
    }

    pub fn deserialize(&self) -> AgentConfigDao {
        let config = self.agent_proxy.deserialize();
        AgentConfigDao::new(
            self.sensor_id,
            self.domain.clone(),
            config.name,
            config.state_json,
        )
    }

    pub fn agent_name(&self) -> &String {
        &self.agent_name
    }

    pub fn domain(&self) -> &String {
        &self.domain
    }

    async fn dispatch_plugin_ipc(
        sensor_id: i32,
        domain: String,
        state_lock: Arc<RwLock<AgentState>>,
        agent_sender: Sender<SensorMessageDto>,
        mut plugin_receiver: Receiver<AgentMessage>,
    ) {
        while let Some(payload) = plugin_receiver.next().await {
            if let AgentMessage::Task(task) = payload {
                info!(APP_LOGGING, "Spawning new task for sensor: {}", sensor_id);
                tokio::spawn(task);
            } else if let AgentMessage::State(state) = payload {
                let mut self_state = state_lock.write().unwrap();
                *self_state = state;
            } else if let Ok(message) = AgentPayload::from(payload) {
                let mut msg = SensorMessageDto {
                    sensor_id: sensor_id,
                    domain: domain.clone(),
                    payload: message,
                };

                let mut tries = 3;
                let mut sender = agent_sender.clone();
                while tries != 0 {
                    if let Err(e) = sender.send(msg).await {
                        error!(APP_LOGGING, "[{}/3] Error sending payload: {}", tries, e);
                        tries -= 1;
                        msg = e.0;
                    } else {
                        break;
                    }
                }
            } else {
                error!(APP_LOGGING, "Unhandled payload");
            }
        }
    }
}

#[derive(Debug)]
pub struct AgentFactory {
    agent_sender: Sender<SensorMessageDto>,
    libraries: HashMap<String, Library>,
}

impl AgentFactory {
    pub fn new(sender: Sender<SensorMessageDto>) -> Self {
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

    unsafe fn load_library<P: AsRef<OsStr>>(
        &mut self,
        library_path: P,
    ) -> Result<&str, PluginError> {
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
            if loaded_decl.agent_version < decl.agent_version {
                return Err(PluginError::Duplicate(decl.agent_name.to_owned()));
            }
        }
        self.libraries.insert(decl.agent_name.to_owned(), library);
        return Ok(decl.agent_name);
    }

    unsafe fn build_proxy_agent(
        &self,
        agent_name: &String,
        state_json: Option<&String>,
        plugin_sender: &Sender<AgentMessage>,
    ) -> Option<Box<dyn AgentTrait>> {
        if let Some(library) = self.libraries.get(agent_name) {
            let decl = library
                .get::<*mut PluginDeclaration>(b"plugin_declaration\0")
                .unwrap() // Panic should be impossible
                .read();

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
        let (plugin_sender, plugin_receiver) = channel::<AgentMessage>(32);
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