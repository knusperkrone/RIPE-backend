use crate::config::CONFIG;
use logging::APP_LOGGING;
use sqlx::migrate::Migrator;

#[macro_use]
extern crate slog;
extern crate yaml_rust;

mod config;
mod error;
mod logging;
mod models;
mod mqtt;
mod plugin;
mod rest;
mod sensor;

static MIGRATOR: Migrator = sqlx::migrate!(); // defaults to "./migrations"

async fn connect_db() -> sqlx::PgPool {
    for i in 0..15 {
        if let Ok(Some(db_conn)) = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            models::establish_db_connection(),
        )
        .await
        {
            if MIGRATOR.run(&db_conn).await.is_ok() {
                info!(APP_LOGGING, "Run migrations");
                return db_conn;
            }
            return db_conn;
        }
        error!(APP_LOGGING, "Couldn't connect to db [{}/15]", i);
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    panic!("No db connection etablished");
}

#[tokio::main]
pub async fn main() -> std::io::Result<()> {
    env_logger::init();
    plugin::agent::register_sigint_handler();

    // Init Observer service
    let plugin_path = CONFIG.plugin_dir();
    let plugin_dir = std::path::Path::new(&plugin_path);
    let db_conn = connect_db().await;
    let sensor_arc = sensor::ConcurrentObserver::new(plugin_dir, db_conn);

    // Prepare daemon tasks for current-thread
    let server_loop = rest::dispatch_server_daemon(sensor_arc.clone());
    let reveice_mqtt_loop =
        sensor::ConcurrentObserver::dispatch_mqtt_receive_loop(sensor_arc.clone());
    let iac_loop = sensor::ConcurrentObserver::dispatch_iac_loop(sensor_arc.clone());
    let plugin_loop = sensor::ConcurrentObserver::dispatch_plugin_refresh_loop(sensor_arc.clone());

    let _ = tokio::join!(
        sensor_arc.init(),
        reveice_mqtt_loop,
        server_loop,
        iac_loop,
        plugin_loop
    );
    Ok(())
}
