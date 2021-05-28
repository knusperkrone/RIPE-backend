use diesel::PgConnection;
use logging::APP_LOGGING;

use crate::config::CONFIG;

#[macro_use]
extern crate diesel;
#[macro_use]
extern crate slog;
#[macro_use]
extern crate diesel_migrations;

mod config;
mod error;
mod logging;
mod models;
mod mqtt;
mod plugin;
mod rest;
mod schema;
mod sensor;

diesel_migrations::embed_migrations!();

fn connect_db() -> PgConnection {
    for i in 0..15 {
        if let Some(db_conn) = models::establish_db_connection() {
            if let Ok(_) = embedded_migrations::run(&db_conn) {
                return db_conn;
            }
        }
        error!(APP_LOGGING, "Couldn't connect to db [{}/15]", i);
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    panic!("No db connection etablished");
}

#[tokio::main(flavor = "current_thread")]
pub async fn main() -> std::io::Result<()> {
    env_logger::init();
    plugin::agent::register_sigint_handler();

    // Init Observer service
    let plugin_path = CONFIG.plugin_dir();
    let plugin_dir = std::path::Path::new(&plugin_path);
    let db_conn = connect_db();
    let sensor_arc = sensor::ConcurrentSensorObserver::new(plugin_dir, db_conn);
    sensor_arc.init().await;

    // Prepare daemon tasks for current-thread
    let reveice_mqtt_loop =
        sensor::ConcurrentSensorObserver::dispatch_mqtt_receive_loop(sensor_arc.clone());
    let iac_loop = sensor::ConcurrentSensorObserver::dispatch_iac_loop(sensor_arc.clone());
    let plugin_loop =
        sensor::ConcurrentSensorObserver::dispatch_plugin_refresh_loop(sensor_arc.clone());

    // Single-thread runtime for mqtt-requests
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();

        // Await mqtt requests in multithread runtime
        runtime.block_on(reveice_mqtt_loop);
    });
    // Multi-thread runtime for rest-requests
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        // Await server requests in multithread runtime
        runtime.block_on(rest::dispatch_server_daemon(sensor_arc));
    });
    // Single-thread runtime for local plugin changes and iac events
    let _ = tokio::join!(iac_loop, plugin_loop);
    Ok(())
}
