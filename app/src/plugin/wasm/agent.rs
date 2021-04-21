use super::{stubs, WasmerMallocPtr};
use crate::{error::WasmPluginError, logging::APP_LOGGING};
use iftem_core::{AgentTrait, AgentUIDecorator};
use parking_lot::Mutex;
use std::collections::HashMap;
use wasmer::{Instance, Memory, NativeFunc};

pub struct WasmAgent {
    pub agent_ptr: i32,
    pub instance: Instance,
    pub error_indicator: Mutex<Option<WasmPluginError>>,
    pub wasm_malloc: NativeFunc<i32, i32>, // size -> memory_ptr
    pub wasm_free: NativeFunc<i32, ()>,    // memory_ptr -> void
    pub wasm_handle_data: NativeFunc<(i32, i32), ()>, // agent_ptr, data_str_ptr -> void
    pub wasm_handle_cmd: NativeFunc<(i32, i64), ()>, // agent_ptr, payload -> void
    pub wasm_render_ui: NativeFunc<(i32, i32), i32>, // agent_ptr, data_str_ptr -> void
    pub wasm_deserialize: NativeFunc<i32, i32>, // agent_ptr -> str_ptr
    pub wasm_state: NativeFunc<i32, i32>,  // agent_ptr -> str_ptr
    pub wasm_cmd: NativeFunc<i32, i32>,    // agent_ptr -> str_ptr
    pub wasm_config: NativeFunc<i32, i32>, // agent_ptr -> str_ptr
    pub wasm_set_config: NativeFunc<(i32, i32), i32>, // agent_ptr, data_str_ptr -> str_ptr
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
        let len = msg.bytes().len() as i32;
        let ptr = self
            .wasm_malloc
            .call(len)
            .map_err(|e| self.inidicate_error(&"write", e))?;
        // write bytes
        let view = memory.view();
        let heap_view = view[ptr as usize..(ptr + len) as usize].iter();
        for (byte, cell) in msg.bytes().zip(heap_view) {
            cell.set(byte);
        }

        Ok(WasmerMallocPtr { agent: self, ptr })
    }

    pub(crate) fn read(&self, ptr: i32) -> Option<String> {
        let memory: &Memory = self.instance.exports.get("memory").unwrap();
        stubs::read_c_str(memory, ptr)
    }

    pub(crate) fn free(&self, ptr: i32) -> Result<(), WasmPluginError> {
        Ok(self.wasm_free.call(ptr)?)
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
    fn handle_data(&mut self, data: &iftem_core::SensorDataMessage) {
        if self.has_error() {
            return;
        }

        let data_json = serde_json::to_string(data).unwrap();
        let alloced = self.write(&data_json).unwrap();

        let _ = self
            .wasm_handle_data
            .call(self.agent_ptr, alloced.ptr)
            .map_err(|e| self.inidicate_error(&"handle_data", e));
    }

    fn handle_cmd(&mut self, payload: i64) {
        if self.has_error() {
            return;
        }

        let _ = self
            .wasm_handle_cmd
            .call(self.agent_ptr, payload)
            .map_err(|e| self.inidicate_error(&"handle_cmd", e));
    }

    fn render_ui(&self, data: &iftem_core::SensorDataMessage) -> iftem_core::AgentUI {
        if self.has_error() {
            return iftem_core::AgentUI {
                decorator: AgentUIDecorator::Text,
                rendered: "Invalid internal state".to_owned(),
                state: iftem_core::AgentState::Error,
            };
        }
        let data_json = serde_json::to_string(data).unwrap();

        if let Ok(alloced) = self.write(&data_json) {
            match self.wasm_render_ui.call(self.agent_ptr, alloced.ptr) {
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

        return iftem_core::AgentUI {
            decorator: AgentUIDecorator::Text,
            rendered: "Failed serializing agent_ui".to_owned(),
            state: iftem_core::AgentState::Error,
        };
    }

    fn state(&self) -> iftem_core::AgentState {
        if !self.has_error() {
            if let Ok(json_ptr) = self.wasm_state.call(self.agent_ptr) {
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
        iftem_core::AgentState::Error
    }

    fn cmd(&self) -> i32 {
        if self.has_error() {
            0
        } else {
            self.wasm_cmd.call(self.agent_ptr).unwrap_or(0)
        }
    }

    fn deserialize(&self) -> String {
        if !self.has_error() {
            if let Ok(json_ptr) = self.wasm_deserialize.call(self.agent_ptr) {
                if let Some(json_str) = self.read(json_ptr) {
                    return json_str;
                }
            }
        }

        self.inidicate_error(&"deserialize", WasmPluginError::CallError);
        "{}".to_owned()
    }

    fn config(&self) -> HashMap<String, (String, iftem_core::AgentConfigType)> {
        type Map = HashMap<String, (String, iftem_core::AgentConfigType)>;
        if !self.has_error() {
            if let Ok(json_ptr) = self.wasm_config.call(self.agent_ptr) {
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

    fn set_config(&mut self, values: &HashMap<String, iftem_core::AgentConfigType>) -> bool {
        if self.has_error() {
            return false;
        }

        let data_json = serde_json::to_string(values).unwrap();
        let alloced = self.write(&data_json).unwrap();

        if let Ok(json_ptr) = self.wasm_set_config.call(self.agent_ptr, alloced.ptr) {
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
