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

#[tokio::main]
pub async fn main() -> std::io::Result<()> {
    env_logger::init();
    let sensor_arc = sensor::ConcurrentSensorObserver::new();
    let mqtt_thread = sensor::ConcurrentSensorObserver::dispatch_mqtt_loop(sensor_arc.clone());
    let iac_loop = sensor::ConcurrentSensorObserver::dispatch_iac_stream(sensor_arc.clone());
    let plugin_loop = sensor::ConcurrentSensorObserver::dispatch_plugin_loop(sensor_arc.clone());
    let server_daemon = rest::dispatch_server_daemon(sensor_arc.clone());
    plugin::agent::register_sigint_handler();

    let _ = tokio::join!(mqtt_thread, iac_loop, plugin_loop, server_daemon);
    Ok(())
}
