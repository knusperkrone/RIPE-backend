use super::{stubs, WasmerMallocPtr};
use crate::{error::WasmPluginError, logging::APP_LOGGING};
use chrono_tz::Tz;
use parking_lot::{Mutex, RwLock};
use ripe_core::{AgentTrait, AgentUIDecorator};
use std::{collections::HashMap, sync::Arc};
use wasmer::{AsStoreMut, AsStoreRef, Instance, Memory, Store, TypedFunction};

pub struct WasmAgent {
    pub instance: Instance,
    pub store: Arc<RwLock<Store>>,
    pub error_indicator: Mutex<Option<WasmPluginError>>,
    pub wasm_malloc: TypedFunction<u64, u64>, // size -> memory_ptr
    pub wasm_free: TypedFunction<u64, ()>,    // memory_ptr -> void
    pub wasm_handle_data: TypedFunction<u64, ()>, // data_str_ptr -> void
    pub wasm_handle_cmd: TypedFunction<i64, ()>, // payload -> void
    pub wasm_render_ui: TypedFunction<u64, u64>, // data_str_ptr -> void
    pub wasm_deserialize: TypedFunction<(), u64>, // () -> str_ptr
    pub wasm_state: TypedFunction<(), u64>,   // () -> str_ptr
    pub wasm_cmd: TypedFunction<(), i32>,     // () -> str_ptr
    pub wasm_config: TypedFunction<(), u64>,  // () -> str_ptr
    pub wasm_set_config: TypedFunction<u64, u64>, // data_str_ptr -> str_ptr
}

impl WasmAgent {
    pub(crate) fn inidicate_error<E>(&self, method: &str, err: E) -> WasmPluginError
    where
        E: std::error::Error + Clone + Into<WasmPluginError>,
    {
        let mut lock = self.error_indicator.lock();
        error!(APP_LOGGING, "WasmAgent has {:?} in {}", err, method);
        *lock = Some(err.clone().into());
        err.into()
    }

    pub(crate) fn write(&self, msg: &str) -> Result<WasmerMallocPtr, WasmPluginError> {
        // malloc
        let memory: &Memory = self.instance.exports.get("memory")?;
        let mut store = self.store.write();

        let len = msg.bytes().len() as u64;
        let ptr = self
            .wasm_malloc
            .call(&mut store.as_store_mut(), len)
            .map_err(|e| self.inidicate_error(&"write", e))? as u64;
        // write bytes
        let bytes = msg.bytes().clone().collect::<Vec<u8>>();
        let view = memory.view(&mut store.as_store_mut());
        let _ = view.write(ptr, &bytes);

        Ok(WasmerMallocPtr { agent: self, ptr })
    }

    pub(crate) fn read(&self, ptr: u64) -> Option<String> {
        let memory: &Memory = self.instance.exports.get("memory").unwrap();
        stubs::read_c_str(&self.store.read().as_store_ref(), memory, ptr)
    }

    pub(crate) fn free(&self, ptr: u64) -> Result<(), WasmPluginError> {
        Ok(self.wasm_free.call(&mut *self.store.write(), ptr)?)
    }

    pub(crate) fn has_error(&self) -> bool {
        let error_guard = self.error_indicator.lock();
        error_guard.is_some()
    }
}

impl std::fmt::Debug for WasmAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WasmerAgent")
    }
}

impl AgentTrait for WasmAgent {
    fn handle_data(&mut self, data: &ripe_core::SensorDataMessage) {
        if self.has_error() {
            return;
        }

        let data_json = serde_json::to_string(data).unwrap();
        let alloced = self.write(&data_json).unwrap();

        let _ = self
            .wasm_handle_data
            .call(&mut *self.store.write(), alloced.ptr)
            .map_err(|e| self.inidicate_error(&"handle_data", e));
    }

    fn handle_cmd(&mut self, payload: i64) {
        if self.has_error() {
            return;
        }

        let _ = self
            .wasm_handle_cmd
            .call(&mut *self.store.write(), payload)
            .map_err(|e| self.inidicate_error(&"handle_cmd", e));
    }

    fn render_ui(&self, data: &ripe_core::SensorDataMessage, _timezone: Tz) -> ripe_core::AgentUI {
        if self.has_error() {
            return ripe_core::AgentUI {
                decorator: AgentUIDecorator::Text,
                rendered: "Invalid internal state".to_owned(),
                state: ripe_core::AgentState::Error,
            };
        }
        let data_json = serde_json::to_string(data).unwrap();

        if let Ok(alloced) = self.write(&data_json) {
            match self
                .wasm_render_ui
                .call(&mut *self.store.write(), alloced.ptr)
            {
                Ok(json_ptr) => {
                    if let Some(json_str) = self.read(json_ptr) {
                        if let Ok(ui) = serde_json::from_str(&json_str) {
                            return ui;
                        }
                    }
                }
                Err(e) => {
                    self.inidicate_error(&"render_ui", e);
                }
            };
        }

        return ripe_core::AgentUI {
            decorator: AgentUIDecorator::Text,
            rendered: "Failed serializing agent_ui".to_owned(),
            state: ripe_core::AgentState::Error,
        };
    }

    fn state(&self) -> ripe_core::AgentState {
        if !self.has_error() {
            if let Ok(json_ptr) = self.wasm_state.call(&mut *self.store.write()) {
                if let Some(json_str) = self.read(json_ptr) {
                    if let Ok(state) = serde_json::from_str(&json_str) {
                        return state;
                    } else {
                        crit!(APP_LOGGING, "Failed serializing: {}", json_str);
                    }
                }
            }
        }
        self.inidicate_error(&"state", WasmPluginError::CallError);
        ripe_core::AgentState::Error
    }

    fn cmd(&self) -> i32 {
        if self.has_error() {
            0
        } else {
            self.wasm_cmd.call(&mut *self.store.write()).unwrap_or(0)
        }
    }

    fn deserialize(&self) -> String {
        if !self.has_error() {
            if let Ok(json_ptr) = self.wasm_deserialize.call(&mut *self.store.write()) {
                if let Some(json_str) = self.read(json_ptr) {
                    return json_str;
                }
            }
        }

        self.inidicate_error(&"deserialize", WasmPluginError::CallError);
        "{}".to_owned()
    }

    fn config(&self, _timezone: Tz) -> HashMap<String, (String, ripe_core::AgentConfigType)> {
        type Map = HashMap<String, (String, ripe_core::AgentConfigType)>;
        if !self.has_error() {
            if let Ok(json_ptr) = self.wasm_config.call(&mut *self.store.write()) {
                if let Some(json_str) = self.read(json_ptr) {
                    if let Ok(ui) = serde_json::from_str::<Map>(&json_str) {
                        return ui;
                    }
                }
            }
        }

        self.inidicate_error(&"config", WasmPluginError::CallError);
        HashMap::new()
    }

    fn set_config(
        &mut self,
        values: &HashMap<String, ripe_core::AgentConfigType>,
        _timezone: Tz,
    ) -> bool {
        if self.has_error() {
            return false;
        }

        let data_json = serde_json::to_string(values).unwrap();
        let alloced = self.write(&data_json).unwrap();

        if let Ok(json_ptr) = self
            .wasm_set_config
            .call(&mut *self.store.write(), alloced.ptr)
        {
            if let Some(json_str) = self.read(json_ptr) {
                if let Ok(success) = serde_json::from_str(&json_str) {
                    return success;
                }
            }
        }
        self.inidicate_error(&"set_config", WasmPluginError::CallError);
        false
    }
}
