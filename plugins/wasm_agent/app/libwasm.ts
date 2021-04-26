import { log } from "./env";
import { Date } from "as-date";

export const NULL: i32 = 0;

export class Tuple2<T1, T2> {
  constructor(public val1: T1, public val2: T2) {}
}


function pad(n: i32): string {
  return n < 10 ? `0${n}` : `${n}`;
}

export function dateToString(d: Date): string {
  const decade: string = `${d.getUTCFullYear()}-${pad(d.getUTCMonth())}-${pad(
    d.getUTCDay()
  )}`;
  const daytime: string = `${pad(d.getUTCHours())}:${pad(
    d.getUTCMinutes()
  )}:${pad(d.getUTCSeconds())}.${pad(d.getUTCMilliseconds())}`;
  return `${decade}T${daytime}Z`;
}

export function ptrToString(ptr: usize): string {
  if (ptr == 0) {
    return "NULL";
  }
  let alloced: Uint8Array = Uint8Array.wrap(changetype<ArrayBuffer>(ptr));

  let strBuffer: string = "";
  for (let i = 0; i < alloced.length; i++) {
    strBuffer += String.fromCharCode(alloced[i]);
  }
  return strBuffer;
}

export function print(value: string): void {
  const encoded: ArrayBuffer = String.UTF8.encode(value, true);
  log(encoded);
}

export function printf(values: string[]): void {
  let strBuffer: string = "";
  for (let i = 0; i < values.length; i++) {
    strBuffer += values[i];
    if (i != values.length - 1) {
      strBuffer += " ";
    }
  }
  const encoded: ArrayBuffer = String.UTF8.encode(strBuffer, true);
  log(encoded);
}

export function escapeJsonString(str: string): string {
  let buffer: string = "";
  let savedIndex = 0;
  for (let i = 0; i < str.length; i++) {
    let char = str.charCodeAt(i);
    let needsEscaping =
      char < 0x20 || char == '"'.charCodeAt(0) || char == "\\".charCodeAt(0);
    if (needsEscaping) {
      buffer += (str.substring(savedIndex, i));
      savedIndex = i + 1;
      if (char == '"'.charCodeAt(0)) {
        buffer += ('\\"');
      } else if (char == "\\".charCodeAt(0)) {
        buffer += ("\\\\");
      } else if (char == "\b".charCodeAt(0)) {
        buffer += ("\\b");
      } else if (char == "\n".charCodeAt(0)) {
        buffer += ("\\n");
      } else if (char == "\r".charCodeAt(0)) {
        buffer += ("\\r");
      } else if (char == "\t".charCodeAt(0)) {
        buffer += ("\\t");
      } else {
        // TODO: Implement encoding for other contol characters
        // @ts-ignore integer does have toString
        assert(
          false,
          "Unsupported control character code: " + char.toString()
        );
      }
    }
  }
  buffer += (str.substring(savedIndex, str.length));
  return buffer;
}