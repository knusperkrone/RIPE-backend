use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::env;

pub struct Config {
    inner: RwLock<InnerConfig>,
}

struct InnerConfig {
    database_url: String,
    mqtt_brokers: Vec<String>,
    plugin_dir: String,
    server_port: String,
    mqtt_index: usize,
}

impl Config {
    pub fn database_url(&self) -> String {
        let inner = self.inner.read();
        inner.database_url.clone()
    }

    pub fn current_mqtt_broker(&self) -> String {
        let inner = self.inner.read();
        inner.mqtt_brokers[inner.mqtt_index].clone()
    }

    pub fn next_mqtt_broker(&self) -> String {
        let mut inner = self.inner.write();
        inner.mqtt_index = (inner.mqtt_index + 1) % inner.mqtt_brokers.len();

        inner.mqtt_brokers[inner.mqtt_index].clone()
    }

    pub fn plugin_dir(&self) -> String {
        let inner = self.inner.read();
        inner.plugin_dir.clone()
    }

    pub fn server_port(&self) -> String {
        let inner = self.inner.read();
        inner.server_port.clone()
    }
}

pub static CONFIG: Lazy<Config> = Lazy::new(|| {
    dotenv::dotenv().expect("Invalid .env file");

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let mqtt_brokers: Vec<String> = env::var("MQTT_BROKERS")
        .expect("MQTT_BROKERS must be set")
        .split(",")
        .map(|s| s.trim().to_owned())
        .collect();
    let plugin_dir = std::env::var("PLUGIN_DIR").expect("PLUGIN_DIR must be set");
    let server_port = env::var("SERVER_PORT").expect("SERVER_PORT must be set");

    if mqtt_brokers.len() == 0 {
        panic!("No MQTT-Brokers provided");
    }

    Config {
        inner: RwLock::new(InnerConfig {
            database_url,
            mqtt_brokers,
            plugin_dir,
            server_port,
            mqtt_index: 0,
        }),
    }
});
