import { log } from "./env";
import { Date } from "as-date";

export const NULL: i32 = 0;


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
