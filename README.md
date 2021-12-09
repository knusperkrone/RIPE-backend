# RIPE

RIPE stands for **R**ust **I**oT **P**lant **E**vents.

This is the backend written in rust.

## Run locally

Install [cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html) and run `cargo run --all`

## Architecture

![UML diagram](https://www.plantuml.com/plantuml/png/TP31IiD048RlynH3Zy8MR7iIALMljhGqBx3P34bmioipcuEex-x6X0H7Uls-NURdEqYodFhEWslPUS8Ntr980E_MlTcQS7LjB5E5H-eYVwpm4n1jAOcjI-Yy5S6YxUYfph_-gpEno-A6BKXkgeP9ckYhoK_uEQ-YKC4t05GssT9AddYEacgcw-LrsFShdOzzDOuD8IQRsXZmU2cAmSwtX894ljTWey5MWvq69m2Onk6ZCLD6V40NG-BeFGaCvt6ztYzI-b8SjsMMo-ylOvQaV_2IKvivfS8guoplZDdZactcneoHngtvqPn8WOq6Mmrs6fpWa4_qdVy1)

The backend communicates via MQTT with the sensor clients and provides a REST interface for the App.

Rust provides a very performant async/await pattern within [tokio crate](https://docs.rs/tokio/1.14.0/tokio/index.html). This allows performant scaling within a **stateful** monolith.

Each sensor has a runtime equivalent inside the monolith. And each action, e.g `water for 5 minutes` is mapped by a job that sends a `water=True` signal, sleeps for 5 minutes and then sends a `water=False` signal.

This co-routine push approach assumes an always running service, that saves the current state.
As updating the application is impossible then. The 'job' logic is done by dynamically loaded plugins.
This libaries are called via the safe rust ABI from inside the shared process memory.

In order to update such a shared libary, it is just necessary to push the new version of the library inside the `plugin` folder and the next job will be executed from the updated and loaded native code.

## Developing

### Monolith

Hopefully you won't as you will use state. Plugins are configured to get into a neutral state, when shutdown
but I'd recommend a very fast restart, after fixing a bug, adding a feature, etc.

### Plugin

Use either the Rust 1.57 compiler and fullfil the rust trait contract or make a WASM module, as described in `plugins/wasm_agent_builder`, the application will do the runtime checking for you.

### High Availabiliy

As the MQTT Servere is an other Single-Point-of-Failure, multiple brokers are supported.
On a connection loss, the next broker is choosen and the clients will fetch/register for the new broker address.
