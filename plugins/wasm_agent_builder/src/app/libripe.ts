import { dateToString, escapeJsonString } from "./libwasm";
import { Date } from "as-date";

/*
 * State
 */

enum _AgentState {
  Disabled,
  Ready,
  Error,
  Executing,
  Stopped,
  Forced,
}

export class AgentState {
  private constructor(private state: _AgentState, private date: Date | null) {}

  static DISABLED(): AgentState {
    return new AgentState(_AgentState.Disabled, null);
  }

  static READY(): AgentState {
    return new AgentState(_AgentState.Ready, null);
  }

  static ERROR(): AgentState {
    return new AgentState(_AgentState.Error, null);
  }

  static EXECUTING(until: Date): AgentState {
    return new AgentState(_AgentState.Error, until);
  }

  static STOPPED(until: Date): AgentState {
    return new AgentState(_AgentState.Stopped, until);
  }

  static FORCED(until: Date): AgentState {
    return new AgentState(_AgentState.Forced, until);
  }

  toJson(): string {
    switch (this.state) {
      case _AgentState.Disabled:
        return '"Disabled"';
      case _AgentState.Ready:
        return '"Ready"';
      case _AgentState.Error:
        return '"Error"';
      case _AgentState.Executing:
        return `{"Executing": ${dateToString(this.date!)}}`;
      case _AgentState.Stopped:
        return `{"Stopped": ${dateToString(this.date!)}}`;
      case _AgentState.Forced:
        return `{"Forced": ${dateToString(this.date!)}}`;
      default:
        return "";
    }
  }
}

/*
 * AgentUI
 */

export class AgentUIDecorator {
  private constructor(private json: string) {}

  static TEXT(): AgentUIDecorator {
    return new AgentUIDecorator('"Text"');
  }

  static TIME_PANE(stepsize: u32): AgentUIDecorator {
    return new AgentUIDecorator(`{"TimePane":${stepsize}}`);
  }

  static SLIDER(lower: f32, upper: f32, val: f32): AgentUIDecorator {
    let json: string = `{"Slider": [${lower}, ${upper}, ${val}}`;
    return new AgentUIDecorator(json);
  }

  toJson(): string {
    return this.json;
  }
}

export class AgentUI {
  constructor(
    public decorator: AgentUIDecorator,
    public state: AgentState,
    public rendered: string
  ) {}

  toJson(): string {
    let decorator: string = this.decorator.toJson();
    let state: string = this.state.toJson();
    let rendered: string = escapeJsonString(this.rendered);
    return `{"decorator":${decorator},"state":${state},"rendered":"${rendered}"}`;
  }
}

/*
 * Config
 */

export class AgentConfigType {
  private constructor(private json: string) {}

  static SWITCH(val: bool): AgentConfigType {
    let json = `{"Switch": ${val}}`;
    return new AgentConfigType(json);
  }

  static DATE_TIME(val: u64): AgentConfigType {
    let json: string = `{"DateTime": ${val}}`;
    return new AgentConfigType(json);
  }

  static INT_PICKER_RANGE(lower: i64, upper: i64, val: i64): AgentConfigType {
    let json: string = `{"IntPickerRange": [${lower}, ${upper}, ${val}]}`;
    return new AgentConfigType(json);
  }

  static INT_SLIDER_RANGE(lower: i64, upper: i64, val: i64): AgentConfigType {
    let json: string = `{"IntSliderRange": [${lower}, ${upper}, ${val}]}`;
    return new AgentConfigType(json);
  }

  static FLOAT_PICKER_RANGE(lower: f64, upper: f64, val: f64): AgentConfigType {
    let json: string = `{"FloatPickerRange": [${lower}, ${upper}, ${val}]}`;
    return new AgentConfigType(json);
  }

  static FLOAT_SLIDER_RANGE(lower: f64, upper: f64, val: f64): AgentConfigType {
    let json: string = `{"FloatSliderRange": [${lower}, ${upper}, ${val}]}`;
    return new AgentConfigType(json);
  }

  toJson(): string {
    return this.json;
  }
}

export class AgentConfig {
  private jsonStr: string = "";

  constructor() {}

  insert(key: string, text: string, uiElement: AgentConfigType): void {
    key = escapeJsonString(key);
    text = escapeJsonString(text);

    this.jsonStr += `"${key}":["${text}",${uiElement.toJson()}]`;
    this.jsonStr += ",";
  }

  toJson(): string {
    let tmpStr: string = this.jsonStr;
    if (tmpStr.length != 0) {
      tmpStr = tmpStr.substring(0, tmpStr.length - 1); // remove trailing comma
    }
    return `{${tmpStr}}`;
  }
}
