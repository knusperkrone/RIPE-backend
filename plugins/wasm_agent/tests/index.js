const assert = require("assert");
const myModule = require("..");

myModule.buildAgent(0);

function readString(ptr) {
    buffer = ''
    while (true) {
        c = new Uint8Array(myModule.memory.buffer)[ptr]
        if (c == undefined || c == 0) {
            break;
        }
        ptr++;
        buffer += String.fromCharCode(c);
    }
    return buffer;
}


stateStr = readString(myModule.getConfig());
console.log(stateStr);
