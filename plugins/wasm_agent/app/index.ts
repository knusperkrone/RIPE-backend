import { dateToString, NULL, print, ptrToString } from "./lib";
import { JSON } from "assemblyscript-json";
import { Date } from "as-date";

/*
 * Your Agent implementation
 */

class Agent {
  private value: i32;

  constructor(oldState: JSON.Obj) {
    const val: JSON.Integer | null = oldState.getInteger("val");
    if (val != null) {
      this.value = 0;
      print("Restored old state");
    } else {
      print("Invalid old state, init new Agent");
    }
  }

  handleCmd(_payload: i64): void {}

  handleData(): void {}

  deserialize(): string {
    return "{}";
  }

  getCmd(): i32 {
    return 0;
  }

  getState(): AgentState {
    return new AgentState(State.Ready);
  }

  renderUI(): string {
    return "{}";
  }

  setConfig(configStr: string): bool {
    return false;
  }

  getConfig(): string {
    return "{}";
  }
}

/*
 * Some shimming code
 * Needs to be in this file, compiler will tree shake otherwise
 * DO NOT ALTER
 */

let INSTANCE: Agent;

enum State {
  Disabled,
  Ready,
  Error,
  Executing,
  Stopped,
  Forced,
}

class AgentState {
  constructor(private state: State, private date: Date | null = null) {}

  toJson(): string {
    switch (this.state) {
      case State.Disabled:
        return '"Disabled"';
      case State.Ready:
        return '"Ready"';
      case State.Error:
        return '"Error"';
      case State.Executing:
        return `{ "Executing": ${dateToString(this.date!)} }`;
      case State.Stopped:
        return `{ "Stopped": ${dateToString(this.date!)} }`;
      case State.Forced:
        return `{ "Forced": ${dateToString(this.date!)} }`;
    }
    return '"Error"';
  }
}

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
export function handleData(agent_ptr: usize, sensor_data_ptr: usize): void {
  const jsonStr = ptrToString(sensor_data_ptr);
  // TODO: transform to SensorMessageObj
  INSTANCE.handleData();
}

// Forward definition - compiler will tree shake otherwise
export function handleCmd(agent_ptr: usize, payload: i64): void {
  INSTANCE.handleCmd(payload);
}

// Forward definition - compiler will tree shake otherwise
export function renderUI(
  agent_ptr: usize,
  sensor_data_ptr: usize
): ArrayBuffer {
  const jsonStr = ptrToString(sensor_data_ptr);
  // TODO: transform to SensorDataObj
  const ret: string = INSTANCE.renderUI();
  return String.UTF8.encode(ret, true);
}

// Forward definition - compiler will tree shake otherwise
export function deserialize(agent_ptr: usize): ArrayBuffer {
  const ret: string = INSTANCE.deserialize();
  return String.UTF8.encode(ret, true);
}

// Forward definition - compiler will tree shake otherwise
export function getState(agent_ptr: usize): ArrayBuffer {
  const ret: AgentState = INSTANCE.getState();
  return String.UTF8.encode(ret.toJson(), false);
}

// Forward definition - compiler will tree shake otherwise
export function getCmd(agent_ptr: usize): i32 {
  return INSTANCE.getCmd();
}

// Forward definition - compiler will tree shake otherwise
export function getConfig(agent_ptr: usize): ArrayBuffer {
  const ret: string = INSTANCE.getConfig();
  return String.UTF8.encode(ret, true);
}

// Forward definition - compiler will tree shake otherwise
export function setConfig(agent_ptr: usize, config_ptr: usize): bool {
  const configStr = ptrToString(config_ptr);
  return INSTANCE.setConfig(configStr);
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
