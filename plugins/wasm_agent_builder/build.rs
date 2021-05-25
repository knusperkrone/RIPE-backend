use std::env;
use std::process::Command;

fn main() {
    let cargo_src = env::var("CARGO_MANIFEST_DIR").unwrap();
    let cargo_out = env::var("OUT_DIR").unwrap();

    let wasm_src = format!("{}/src", cargo_src);
    let wasm_target = format!("{}/target/untouched.wasm", wasm_src);
    let wasm_out = format!("{}/../../..", cargo_out);

    Command::new("npm")
        .args(&["install", "--prefix", &wasm_src])
        .status()
        .unwrap();

    Command::new("npm")
        .args(&["run", "asbuild:untouched", "--prefix", &wasm_src])
        .status()
        .unwrap();

    Command::new("cp")
        .args(&[wasm_target, wasm_out])
        .status()
        .unwrap();
}
