use crate::error::PluginError;
use crate::logging::APP_LOGGING;
use crate::{models::dao::AgentConfigDao, sensor::handle::SensorHandleMessage};
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
    fmt::Debug,
    string::String,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, RwLock,
    },
};
use tokio::stream::StreamExt;
use tokio::sync::mpsc::{channel, Receiver, Sender, UnboundedSender};

static TERMINATED: AtomicBool = AtomicBool::new(false);
static TASK_COUNTER: AtomicU32 = AtomicU32::new(0);

pub fn register_sigint_handler() {
    // Set termination handler
    ctrlc::set_handler(|| {
        TERMINATED.store(true, Ordering::Relaxed);
        let task_count = TASK_COUNTER.load(Ordering::Relaxed);
        if task_count == 0 {
            std::process::exit(0);
        }
        info!(APP_LOGGING, "Waiting for {} tasks to finish", task_count);
    })
    .unwrap();
}

pub struct Agent {
    sensor_id: i32,
    domain: String,
    plugin_sender: Sender<AgentMessage>,
    agent_name: String,
    abort_handle: AbortHandle,
    agent_proxy: Box<dyn AgentTrait>,
    needs_update: bool,
    library_ref: Arc<Library>, // Reference counter, to not free lib .text section
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

/// A agent, is the lower level wrapper around a AgentTrait implementation
/// It holds all necessary meta-information, for sending messages to the sensor
/// All messages, are handled via the iac-stream
impl Agent {
    pub fn new(
        agent_sender: UnboundedSender<SensorHandleMessage>,
        plugin_sender: Sender<AgentMessage>,
        plugin_receiver: Receiver<AgentMessage>,
        sensor_id: i32,
        domain: String,
        agent_name: String,
        agent_proxy: Box<dyn AgentTrait>,
        library_ref: Arc<Library>,
    ) -> Self {
        let state = Arc::new(RwLock::new(AgentState::Default));

        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        let future = Abortable::new(
            Agent::dispatch_iac(
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
            library_ref,
            needs_update: false,
        }
    }

    pub fn reload_agent(&mut self, factory: &AgentFactory) -> Result<(), PluginError> {
        if self.agent_proxy.state() == AgentState::Active {
            info!(
                APP_LOGGING,
                "Failed reloading active agent: {}", self.sensor_id
            );
            return Err(PluginError::AgentStateError(AgentState::Active));
        }

        info!(
            APP_LOGGING,
            "Reloading agent: {} - {}", self.sensor_id, self.domain
        );
        let state_json = self.agent_proxy.deserialize().state_json;
        unsafe {
            let (libary, proxy) = factory
                .build_proxy_agent(&self.agent_name, Some(&state_json), &self.plugin_sender)
                .ok_or_else(|| PluginError::Duplicate(self.agent_name.clone()))?;
            self.library_ref = libary;
            self.agent_proxy = proxy;
        }
        self.needs_update = false;
        Ok(())
    }

    pub fn on_force(&mut self, active: bool, duration: chrono::Duration) {
        info!(
            APP_LOGGING,
            "Forcing agent: {} - {}", self.sensor_id, self.domain
        );
        self.agent_proxy.on_force(active, duration);
    }

    pub fn on_data(&mut self, data: &SensorDataMessage) {
        self.agent_proxy.on_data(data);
    }

    pub fn cmd(&self) -> i32 {
        self.agent_proxy.cmd()
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

    pub fn set_needs_update(&mut self, needs_update: bool) {
        self.needs_update = needs_update;
    }

    pub fn needs_update(&self) -> bool {
        self.needs_update
    }

    async fn dispatch_iac(
        sensor_id: i32,
        domain: String,
        state_lock: Arc<RwLock<AgentState>>,
        agent_sender: UnboundedSender<SensorHandleMessage>,
        mut plugin_receiver: Receiver<AgentMessage>,
    ) {
        while let Some(payload) = plugin_receiver.next().await {
            if let AgentMessage::Task(agent_task) = payload {
                if TERMINATED.load(Ordering::Relaxed) {
                    info!(APP_LOGGING, "Task wasn't started as app is cancelled");
                } else {
                    tokio::task::spawn(async move {
                        let mut task_count = TASK_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
                        info!(
                            APP_LOGGING,
                            "Spawning new task for sensor: {} - active tasks: {}",
                            sensor_id,
                            task_count
                        );

                        agent_task.await;

                        task_count = TASK_COUNTER.fetch_sub(1, Ordering::Relaxed) - 1;
                        info!(
                            APP_LOGGING,
                            "Ended new task for sensor: {} - active tasks: {}",
                            sensor_id,
                            task_count
                        );
                        if TERMINATED.load(Ordering::Relaxed) && task_count == 0 {
                            std::process::exit(0);
                        }
                    });
                }
            } else if let AgentMessage::State(state) = payload {
                let mut self_state = state_lock.write().unwrap();
                *self_state = state;
            } else if let AgentMessage::Command(command) = payload {
                // Notify main loop over agent
                let mut msg = SensorHandleMessage {
                    sensor_id: sensor_id,
                    domain: domain.clone(),
                    payload: command,
                };

                let mut tries = 3;
                while tries != 0 {
                    if let Err(e) = agent_sender.send(msg) {
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
    agent_sender: UnboundedSender<SensorHandleMessage>,
    libraries: HashMap<String, Arc<Library>>,
}

impl AgentFactory {
    pub fn new(sender: UnboundedSender<SensorHandleMessage>) -> Self {
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
                            Ok(lib_name) => {
                                info!(APP_LOGGING, "Loaded plugin {:?}", path);
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
    ) -> Result<String, PluginError> {
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
                return Err(PluginError::Duplicate(decl.agent_name.to_owned()));
            }
        }
        self.libraries
            .insert(decl.agent_name.to_owned(), Arc::new(library));
        return Ok(decl.agent_name.to_owned());
    }

    unsafe fn build_proxy_agent(
        &self,
        agent_name: &String,
        state_json: Option<&String>,
        plugin_sender: &Sender<AgentMessage>,
    ) -> Option<(Arc<Library>, Box<dyn AgentTrait>)> {
        if let Some(library) = self.libraries.get(agent_name) {
            let decl = library
                .get::<*mut PluginDeclaration>(b"plugin_declaration\0")
                .unwrap() // Panic should be impossible
                .read();

            let logger: &slog::Logger = once_cell::sync::Lazy::force(&APP_LOGGING);
            let proxy = (decl.agent_builder)(state_json, logger.clone(), plugin_sender.clone());
            Some((library.clone(), proxy))
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
                Arc::new(Library::new("./libtest_agent.so").unwrap()),
            ));
        }

        if let Some((plugin_library, plugin_agent)) =
            self.build_proxy_agent(agent_name, state_json, &plugin_sender)
        {
            Ok(Agent::new(
                self.agent_sender.clone(),
                plugin_sender,
                plugin_receiver,
                sensor_id,
                domain.clone(),
                agent_name.clone(),
                plugin_agent,
                plugin_library,
            ))
        } else {
            Err(AgentError::InvalidIdentifier(agent_name.clone()))
        }
    }
}
