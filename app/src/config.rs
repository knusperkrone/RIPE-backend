use once_cell::sync::Lazy;
use std::sync::Arc;
use yaml_rust::{Yaml, YamlLoader};

use crate::mqtt::{Broker, Credentials};

pub struct Config {
    inner: Arc<InnerConfig>,
}

pub struct MappedBroker {
    pub internal: Broker,
    pub external: Broker,
}

struct InnerConfig {
    database_url: String,
    brokers: Vec<MappedBroker>,
    plugin_dir: String,
    server_port: String,
    mqtt_timeout_ms: u64,
    mqtt_send_retries: usize,
    mqtt_log_count: i64,
}

impl Config {
    pub fn database_url(&self) -> &String {
        &self.inner.database_url
    }

    pub fn brokers(&self) -> &Vec<MappedBroker> {
        &self.inner.brokers
    }

    pub fn mqtt_timeout_ms(&self) -> u64 {
        self.inner.mqtt_timeout_ms
    }

    pub fn mqtt_send_retries(&self) -> usize {
        self.inner.mqtt_send_retries
    }

    pub fn mqtt_log_count(&self) -> i64 {
        self.inner.mqtt_log_count
    }

    pub fn plugin_dir(&self) -> &String {
        &self.inner.plugin_dir
    }

    pub fn server_port(&self) -> &String {
        &self.inner.server_port
    }
}

impl From<Yaml> for Broker {
    fn from(yaml: Yaml) -> Self {
        let uri_str = yaml["uri"]
            .as_str()
            .expect("mqtt.brokers.internal|external.uri must be set");
        let uri = uri_str.parse::<warp::http::Uri>().expect("Invalid uri");
        match uri.scheme_str() {
            Some("tcp") | Some("wss") => (),
            _ => panic!("mqtt.brokers.uri must be specify tcp or wss scheme."),
        }

        let username = yaml["username"].as_str();
        let password = yaml["password"].as_str();
        match (username, password) {
            (Some(username), Some(password)) => Broker {
                uri: uri,
                credentails: Some(Credentials {
                    username: username.to_owned(),
                    password: password.to_owned(),
                }),
            },
            (None, None) => Broker {
                uri: uri,
                credentails: None,
            },
            _ => panic!("mqtt.brokers.username and password must be set together."),
        }
    }
}

impl From<Yaml> for MappedBroker {
    fn from(yaml: Yaml) -> Self {
        let internal = yaml["internal"];
        let external = yaml["external"];
        MappedBroker {
            internal: internal.into(),
            external: external.into(),
        }
    }
}

impl From<Yaml> for Config {
    fn from(yaml: Yaml) -> Self {
        let database_url = yaml["database"]["url"]
            .as_str()
            .expect("database.url must be set");
        let plugin_dir = yaml["plugin"]["dir"]
            .as_str()
            .expect("plugin.dir must be set");
        let server_port = yaml["server"]["port"]
            .as_str()
            .expect("server.port must be set");

        let mqtt_timeout_ms = yaml["mqtt"]["timeout_ms"]
            .as_i64()
            .map(|s| s.to_owned())
            .expect("mqtt.timeout_ms must be set.") as u64;
        let mqtt_log_count = yaml["mqtt"]["log_count"]
            .as_i64()
            .map(|s| s.to_owned())
            .expect("mqtt.log_count must be set.");
        let mqtt_send_retries = yaml["mqtt"]["send_retries"]
            .as_i64()
            .map(|s| s.to_owned())
            .expect("mqtt.send_retries must be set.") as usize;

        let brokers = yaml["mqtt"]["brokers"]
            .as_vec()
            .expect("mqtt.brokers must be set.")
            .iter()
            .map(ToOwned::to_owned)
            .map(MappedBroker::from)
            .collect();

        Config {
            inner: Arc::new(InnerConfig {
                plugin_dir: plugin_dir.to_owned(),
                server_port: server_port.to_owned(),
                database_url: database_url.to_string(),
                brokers,
                mqtt_timeout_ms,
                mqtt_send_retries,
                mqtt_log_count,
            }),
        }
    }
}

pub static CONFIG: Lazy<Config> = Lazy::new(|| {
    let docs = &YamlLoader::load_from_str(
        &std::fs::read_to_string("config.yaml").expect("Couldn't read config.yaml"),
    )
    .expect("Couldn't parse config.yaml")[0];

    Config::from(docs.clone())
});
