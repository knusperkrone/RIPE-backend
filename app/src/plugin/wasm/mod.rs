mod agent;
mod ticker;

use crate::{error::WasmPluginError, logging::APP_LOGGING, sensor::handle::SensorMQTTCommand};
use parking_lot::{Mutex, RwLock};
use ripe_core::{error::AgentError, AgentMessage, AgentTrait, SensorDataMessage};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::{collections::HashMap, ffi::OsStr};
use ticker::TimerFuture;
use tokio::sync::mpsc::{channel, Receiver, Sender, UnboundedSender};
use wasmer::{
    imports, wat2wasm, AsStoreMut, Function, FunctionEnv, Instance, Memory, MemoryType, Module,
    Store, TypedFunction, WasmTypeList,
};

use self::agent::WasmAgent;

use super::{Agent, AgentFactoryTrait, AgentLib};

#[derive(Clone)]
pub struct WasmerInstanceCallEnv {
    sensor_id: i32,
    agent_name: String,
    // sender: Sender<AgentMessage>,
    is_test: bool,
    memory: Memory,
    store: Arc<RwLock<Store>>,
}

pub(crate) struct WasmerMallocPtr<'a> {
    agent: &'a WasmAgent,
    ptr: u64,
}

impl<'a> Drop for WasmerMallocPtr<'a> {
    fn drop(&mut self) {
        let _ = self.agent.free(self.ptr);
    }
}

pub struct WasmAgentFactory {
    store: Arc<RwLock<Store>>,
    libraries: HashMap<String, Arc<Module>>, // <agent_name, Module>
    agent_sender: UnboundedSender<SensorMQTTCommand>,
}

impl AgentFactoryTrait for WasmAgentFactory {
    fn create_agent(
        &self,
        sensor_id: i32,
        agent_name: &str,
        domain: &str,
        state_json: Option<&str>,
        plugin_sender: Sender<AgentMessage>,
        plugin_receiver: Receiver<AgentMessage>,
    ) -> Result<Agent, AgentError> {
        let module = self
            .libraries
            .get(agent_name)
            .ok_or(AgentError::InvalidIdentifier(agent_name.to_owned()))?;

        let proxy = self
            .build_wasm_agent(
                module,
                plugin_sender,
                sensor_id,
                agent_name,
                state_json,
                false,
            )
            .unwrap_or(Err(AgentError::InvalidConfig(
                "Config was invalid".to_owned(),
            ))?);

        Ok(Agent::new(
            self.agent_sender.clone(),
            plugin_receiver,
            sensor_id,
            domain.to_owned(),
            AgentLib(module.clone()),
            agent_name.to_string(),
            Box::new(proxy),
        ))
    }

    fn agents(&self) -> Vec<&String> {
        self.libraries.keys().collect()
    }

    fn load_plugin_file(&mut self, path: &Path) -> Option<String> {
        let ext_res = path.extension();
        let stem_res = path.file_stem();
        if ext_res.is_none() || stem_res.is_none() {
            return None;
        }

        let ext = ext_res?;
        let agent_name = stem_res.unwrap().to_str()?;
        if ext == "wasm" || ext == "wat" {
            let filename = path.as_os_str().to_str()?;
            let bytes_res = WasmAgentFactory::read_wasm_bytes(filename, ext);
            if bytes_res.is_err() {
                return None;
            }

            let bytes = bytes_res.unwrap();
            return match self.load_wasm_file(bytes, agent_name) {
                Ok(_) => {
                    info!(APP_LOGGING, "Loaded wasm {}", filename);
                    Some(agent_name.to_owned())
                }
                Err(err) => {
                    warn!(APP_LOGGING, "Invalid wasm {:?}: {}", filename, err);
                    None
                }
            };
        }
        None
    }
}

impl WasmAgentFactory {
    pub fn new(sender: UnboundedSender<SensorMQTTCommand>) -> Self {
        WasmAgentFactory {
            store: Arc::new(RwLock::new(Store::default())),
            libraries: HashMap::new(),
            agent_sender: sender,
        }
    }

    pub fn has_agent(&self, agent_name: &str) -> bool {
        self.libraries.contains_key(agent_name)
    }

    fn load_wasm_file(&mut self, bytes: Vec<u8>, agent_name: &str) -> Result<(), WasmPluginError> {
        if self.libraries.contains_key(agent_name) {
            return Err(WasmPluginError::Duplicate);
        }

        let module = Module::new(&self.store.write().as_store_mut(), bytes)?;
        let (mock_sender, _mock_receiver) = channel::<AgentMessage>(64);
        let test_agent = self.build_wasm_agent(&module, mock_sender, 0, agent_name, None, true)?;
        if self.test_agent_contract(test_agent) {
            self.libraries
                .insert(agent_name.to_owned(), Arc::new(module));
            Ok(())
        } else {
            Err(WasmPluginError::ContractMismatch(
                "Implementation".to_owned(),
            ))
        }
    }

    fn test_agent_contract(&self, mut agent: WasmAgent) -> bool {
        agent.cmd();
        agent.config(chrono_tz::UTC);
        agent.deserialize();
        agent.handle_cmd(0);
        agent.handle_data(&SensorDataMessage::default());
        agent.render_ui(&SensorDataMessage::default(), chrono_tz::UTC);
        agent.state();

        !agent.has_error()
    }

    fn build_wasm_agent(
        &self,
        module: &Module,
        _sender: Sender<AgentMessage>,
        sensor_id: i32,
        agent_name: &str,
        old_state_opt: Option<&str>,
        is_test: bool,
    ) -> Result<WasmAgent, WasmPluginError> {
        // setup wasmer env
        let env = FunctionEnv::new(
            &mut self.store.write().as_store_mut(),
            WasmerInstanceCallEnv {
                sensor_id,
                // sender,
                is_test,
                agent_name: agent_name.to_owned(),
                store: self.store.clone(),
                memory: Memory::new(
                    &mut self.store.write().as_store_mut(),
                    MemoryType::new(1, None, true),
                )
                .expect("Failed to init wasmer memory"),
            },
        );
        let mut store = self.store.write();
        let import_object = imports! {
            "env" => {
                "abort" => Function::new_typed_with_env(&mut store.as_store_mut(), &env, stubs::abort),
                "sleep" => Function::new_typed_with_env(&mut store.as_store_mut(), &env, stubs::sleep),
                "log" => Function::new_typed_with_env(&mut store.as_store_mut(), &env, stubs::log),
            }
        };
        let instance = Instance::new(&mut store.as_store_mut(), module, &import_object)?;

        // check if module fullfills contract

        let malloc = self.get_native_fn(&instance, "malloc")?;
        let free = self.get_native_fn(&instance, "free")?;
        let handle_data = self.get_native_fn(&instance, "handleData")?;
        let handle_cmd = self.get_native_fn(&instance, "handleCmd")?;
        let render_ui = self.get_native_fn(&instance, "renderUI")?;
        let deserialize = self.get_native_fn(&instance, "deserialize")?;
        let state = self.get_native_fn(&instance, "getState")?;
        let cmd = self.get_native_fn(&instance, "getCmd")?;
        let config = self.get_native_fn(&instance, "getConfig")?;
        let set_config = self.get_native_fn(&instance, "setConfig")?;
        let build_agent = self.get_native_fn::<u64, u64>(&instance, "buildAgent")?;
        let agent = WasmAgent {
            instance,
            store: self.store.clone(),
            error_indicator: Mutex::new(None),
            wasm_malloc: malloc,
            wasm_free: free,
            wasm_handle_data: handle_data,
            wasm_handle_cmd: handle_cmd,
            wasm_render_ui: render_ui,
            wasm_deserialize: deserialize,
            wasm_state: state,
            wasm_cmd: cmd,
            wasm_config: config,
            wasm_set_config: set_config,
        };

        // init wasmer_agent with prev_state
        if let Some(old_state) = old_state_opt {
            let alloced = agent.write(old_state)?;
            build_agent.call(&mut store.as_store_mut(), alloced.ptr)?
        } else {
            build_agent.call(&mut store.as_store_mut(), 0)?
        };

        Ok(agent)
    }

    fn get_native_fn<Args, Rets>(
        &self,
        instance: &Instance,
        name: &str,
    ) -> Result<TypedFunction<Args, Rets>, WasmPluginError>
    where
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        instance
            .exports
            .get_function(name)
            .or(Err(WasmPluginError::ContractMismatch(format!(
                "Functionn {} not implemented",
                name
            ))))?
            .typed::<Args, Rets>(&*self.store.write())
            .map_err(|e| WasmPluginError::ContractMismatch(format!("{} - {}", name, e,)))
    }

    fn read_wasm_bytes(filename: &str, ext: &OsStr) -> Result<Vec<u8>, std::io::Error> {
        let mut f = File::open(filename)?;
        let metadata = fs::metadata(filename)?;
        let mut buffer = vec![0; metadata.len() as usize];
        let _ = f.read(&mut buffer)?;

        if ext == "wasm" {
            if let Ok(bytes) = wat2wasm(&buffer) {
                Ok(bytes.into_owned())
            } else {
                use std::io::{Error, ErrorKind};
                Err(Error::from(ErrorKind::InvalidData))
            }
        } else {
            Ok(buffer)
        }
    }
}

mod stubs {
    use wasmer::{AsStoreRef, FunctionEnvMut};

    use crate::plugin::logging::PLUGIN_LOGGING;

    use super::*;

    pub fn abort(
        env: FunctionEnvMut<WasmerInstanceCallEnv>,
        msg_ptr: u64,
        filename_ptr: u64,
        line_nr: i32,
        col_nr: i32,
    ) {
        let env = env.data();
        let store = env.store.read();
        let msg = read_c_str(&store.as_store_ref(), &env.memory, msg_ptr).unwrap_or_default();
        let filename =
            read_c_str(&store.as_store_ref(), &env.memory, filename_ptr).unwrap_or_default();
        error!(
            PLUGIN_LOGGING,
            "sensor[{}][{}] ABORT with {} at {}:{}:{}",
            env.sensor_id,
            env.agent_name,
            msg,
            filename,
            line_nr,
            col_nr,
        );
    }

    pub fn log(env: FunctionEnvMut<WasmerInstanceCallEnv>, ptr: u64) {
        let env = env.data();
        if !env.is_test {
            return;
        }

        if let Some(msg) = read_c_str(&env.store.read().as_store_ref(), &env.memory, ptr) {
            debug!(
                PLUGIN_LOGGING,
                "sensor[{}][{}] log: {}", env.sensor_id, env.agent_name, msg
            );
        } else {
            warn!(
                PLUGIN_LOGGING,
                "sensor[{}][{}] invalid msg buffer!", env.sensor_id, env.agent_name
            );
        }
    }

    pub fn sleep(env: FunctionEnvMut<WasmerInstanceCallEnv>, ms: u64) {
        let env = env.data();
        debug!(
            PLUGIN_LOGGING,
            "sensor[{}][{}] sleep for {}", env.sensor_id, env.agent_name, ms
        );
        futures::executor::block_on(async move {
            TimerFuture::new(std::time::Duration::from_millis(ms)).await
        });
    }

    pub fn read_c_str(store: &impl AsStoreRef, memory: &Memory, mut ptr: u64) -> Option<String> {
        let view = memory.view(store);
        let mut buffer = Vec::with_capacity(128);
        while let Ok(byte) = view.read_u8(ptr) {
            if byte == 0 {
                break;
            }
            buffer.push(byte);
            ptr += 1;
        }

        if let Ok(str) = std::str::from_utf8(&buffer) {
            Some(str.to_owned())
        } else {
            None
        }
    }
}
