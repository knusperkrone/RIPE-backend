import { NULL, print, ptrToString } from "./libwasm";
import { JSON, JSONEncoder } from "assemblyscript-json";
import {
  AgentConfig,
  AgentConfigType,
  AgentState,
  AgentUI,
  AgentUIDecorator,
} from "./libripe";

/*
 * Your Agent implementation
 */

class Agent {
  private value: i32 = 0;
  private configValue: bool = false;

  constructor(oldState: JSON.Obj) {
    const val: JSON.Integer | null = oldState.getInteger("val");
    const configVal: JSON.Bool | null = oldState.getBool("configVal");
    if (val != null && configVal != null) {
      this.value = val.valueOf() as i32;
      this.configValue = configVal.valueOf();
      print("Restored old state");
    } else {
      print("Invalid old state, init new Agent");
    }
  }

  handleCmd(payload: i64): void {
    this.value = payload as i32;
  }

  handleData(): void {
    // TODO:
  }

  deserialize(): string {
    let encoder = new JSONEncoder();
    encoder.setInteger("val", this.value);

    return encoder.toString();
  }

  getCmd(): i32 {
    return this.value;
  }

  getState(): AgentState {
    return AgentState.READY();
  }

  renderUI(): AgentUI {
    return new AgentUI(
      AgentUIDecorator.SLIDER(0.0, 20.0, this.value as f32),
      this.getState(),
      "Hello from your WASM plugin"
    );
  }

  setConfig(config: JSON.Obj): bool {
    let keyValue: JSON.Bool | null = config.getBool("key");
    if (keyValue == null) {
      return false;
    }
    this.configValue = keyValue.valueOf() as bool;

    return true;
  }

  getConfig(): AgentConfig {
    let config = new AgentConfig();
    config.insert("key", "Deine Einstellung", AgentConfigType.SWITCH(this.configValue));

    return config;
  }
}

/*
 * Shimming code
 * Needs to be in this file, compiler will tree shake otherwise
 * DO NOT ALTER
 */

export let INSTANCE: Agent;

// Forward definition - compiler will tree shake otherwise
export function malloc(size: usize): ArrayBuffer {
  const alloced: Uint8Array = new Uint8Array(size as i32);
  return alloced.buffer;
}

// Forward definition - compiler will tree shake otherwise
export function free(ptr: usize): void {
  let alloced: Uint8Array = Uint8Array.wrap(changetype<ArrayBuffer>(ptr));
  changetype<usize>(alloced);
}

// Forward definition - compiler will tree shake otherwise
export function handleData(sensor_data_ptr: usize): void {
  const jsonStr = ptrToString(sensor_data_ptr);
  // TODO: transform to SensorMessageObj
  INSTANCE.handleData();
}

// Forward definition - compiler will tree shake otherwise
export function handleCmd(payload: i64): void {
  INSTANCE.handleCmd(payload);
}

// Forward definition - compiler will tree shake otherwise
export function renderUI(sensor_data_ptr: usize): ArrayBuffer {
  const jsonStr = ptrToString(sensor_data_ptr);
  // TODO: transform to SensorDataObj
  const ret: string = INSTANCE.renderUI().toJson();
  return String.UTF8.encode(ret, true);
}

// Forward definition - compiler will tree shake otherwise
export function deserialize(): ArrayBuffer {
  const ret: string = INSTANCE.deserialize();
  return String.UTF8.encode(ret, true);
}

// Forward definition - compiler will tree shake otherwise
export function getState(): ArrayBuffer {
  const ret: AgentState = INSTANCE.getState();
  return String.UTF8.encode(ret.toJson(), false);
}

// Forward definition - compiler will tree shake otherwise
export function getCmd(): i32 {
  return INSTANCE.getCmd();
}

// Forward definition - compiler will tree shake otherwise
export function getConfig(): ArrayBuffer {
  const ret: string = INSTANCE.getConfig().toJson();
  return String.UTF8.encode(ret, true);
}

// Forward definition - compiler will tree shake otherwise
export function setConfig(config_ptr: usize): bool {
  const configJson = ptrToString(config_ptr);
  let jsonObj: JSON.Obj = <JSON.Obj>JSON.parse(configJson);
  return INSTANCE.setConfig(jsonObj);
}

// Forward definition - compiler will tree shake otherwise
export function buildAgent(state_ptr: usize): usize {
  let jsonObj: JSON.Obj = new JSON.Obj();
  if (state_ptr != NULL) {
    const json: string = ptrToString(state_ptr);
    jsonObj = <JSON.Obj>JSON.parse(json);
  }
  INSTANCE = new Agent(jsonObj);

  // agent must be pinned
  //const agent = new Agent(jsonObj);
  return changetype<usize>(INSTANCE);
}
