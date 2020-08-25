#[macro_use]
extern crate diesel;
#[macro_use]
extern crate slog;

mod error;
mod logging;
mod models;
mod mqtt;
mod plugin;
mod rest;
mod schema;
mod sensor;

#[actix_rt::main]
async fn main() {
    let sensor_arc = sensor::ConcurrentSensorObserver::new();
    let dispatch_mqtt = sensor::ConcurrentSensorObserver::dispatch_mqtt(sensor_arc.clone());
    let dispatch_ipc = sensor::ConcurrentSensorObserver::dispatch_ipc(sensor_arc.clone());
    let dispatch_plugin = sensor::ConcurrentSensorObserver::dispatch_plugin(sensor_arc.clone());
    let dispatch_rest = rest::dispatch_server(sensor_arc.clone());

    tokio::join!(dispatch_mqtt, dispatch_ipc, dispatch_plugin, dispatch_rest);
}
