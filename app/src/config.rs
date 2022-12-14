use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::env;

pub struct Config {
    inner: RwLock<InnerConfig>,
}

struct InnerConfig {
    database_url: String,
    mqtt_broker_tcp: String,
    mqtt_broker_wss: String,
    plugin_dir: String,
    server_port: String,
    mqtt_timeout_ms: u64,
    mqtt_send_retries: usize,
    mqtt_log_count: i64,
}

impl Config {
    pub fn database_url(&self) -> String {
        let inner = self.inner.read();
        inner.database_url.clone()
    }

    pub fn mqtt_broker_tcp(&self) -> String {
        let inner = self.inner.read();
        inner.mqtt_broker_tcp.clone()
    }

    pub fn mqtt_broker_wss(&self) -> String {
        let inner = self.inner.read();
        inner.mqtt_broker_wss.clone()
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
    let mqtt_broker_tcp = env::var("MQTT_BROKER_TCP").expect("MQTT_BROKER_TCP must be set");
    let mqtt_broker_wss = env::var("MQTT_BROKER_WSS").expect("MQTT_BROKER_WSS must be set");
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

    Config {
        inner: RwLock::new(InnerConfig {
            server_port,
            database_url,
            plugin_dir,
            mqtt_broker_tcp,
            mqtt_broker_wss,
            mqtt_timeout_ms,
            mqtt_log_count,
            mqtt_send_retries,
        }),
    }
});
