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
    mqtt_timeout_ms: u64,
    mqtt_send_retries: usize,
    mqtt_log_count: i64,
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

    pub fn mqtt_timeout_ms(&self) -> u64 {
        self.inner.read().mqtt_timeout_ms
    }

    pub fn mqtt_send_retries(&self) -> usize {
        self.inner.read().mqtt_send_retries
    }

    pub fn mqtt_log_count(&self) -> i64 {
        self.inner.read().mqtt_log_count
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

    let server_port = env::var("SERVER_PORT").expect("SERVER_PORT must be set");
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let plugin_dir = std::env::var("PLUGIN_DIR").expect("PLUGIN_DIR must be set");
    let mqtt_brokers: Vec<String> = env::var("MQTT_BROKERS")
        .expect("MQTT_BROKERS must be set")
        .split(",")
        .map(|s| s.trim().to_owned())
        .collect();
    let mqtt_timeout_ms = std::env::var("MQTT_TIMEOUT_MS")
        .expect("MQTT_TIMEOUT_MS must be set")
        .parse()
        .unwrap();
    let mqtt_log_count = std::env::var("MQTT_LOG_COUNT")
        .expect("MQTT_LOG_COUNT must be set")
        .parse()
        .unwrap();
    let mqtt_send_retries = std::env::var("MQTT_SEND_RETRIES")
        .expect("MQTT_SEND_RETRIES must be set")
        .parse()
        .unwrap();

    if mqtt_brokers.len() == 0 {
        panic!("No MQTT-Brokers provided");
    }

    Config {
        inner: RwLock::new(InnerConfig {
            server_port,
            database_url,
            plugin_dir,
            mqtt_brokers,
            mqtt_timeout_ms,
            mqtt_log_count,
            mqtt_send_retries,
            mqtt_index: 0,
        }),
    }
});
