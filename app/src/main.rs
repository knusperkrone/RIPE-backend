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

    let db_conn = models::establish_db_connection();
    let sensor_arc = sensor::ConcurrentSensorObserver::new(db_conn);
    sensor_arc.load_plugins();

    let reveice_mqtt_loop =
        sensor::ConcurrentSensorObserver::dispatch_mqtt_receive_loop(sensor_arc.clone());
    let send_mqtt_loop =
        sensor::ConcurrentSensorObserver::dispatch_mqtt_send_loop(sensor_arc.clone());
    let plugin_loop =
        sensor::ConcurrentSensorObserver::dispatch_plugin_refresh_loop(sensor_arc.clone());
    let server_daemon = rest::dispatch_server_daemon(sensor_arc.clone());
    plugin::agent::register_sigint_handler();

    //server_daemon.await;
    let _ = tokio::join!(
        reveice_mqtt_loop,
        send_mqtt_loop,
        plugin_loop,
        server_daemon
    );
    Ok(())
}
