use crate::config::CONFIG;
use sqlx::migrate::Migrator;
use tracing::{error, info, Level};

extern crate yaml_rust;

mod config;
mod error;
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
            match MIGRATOR.run(&db_conn).await {
                Ok(_) => info!("Migrations run successfully"),
                Err(e) => {
                    error!("Failed to run migrations: {:?}", e);
                }
            }
            return db_conn;
        }
        error!("Couldn't connect to db [{}/15]", i);
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    panic!("No db connection etablished");
}

#[tokio::main]
pub async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();
    plugin::agent::register_sigint_handler();

    info!("Starting up");
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
