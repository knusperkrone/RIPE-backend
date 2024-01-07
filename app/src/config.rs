use once_cell::sync::Lazy;
use std::sync::Arc;
use yaml_rust::{Yaml, YamlLoader};

use crate::mqtt::{BrokerCredentials, MqttBroker, MqttConnectionDetail, MqttScheme};

pub struct Config {
    inner: Arc<InnerConfig>,
}

pub struct MappedBroker {
    pub internal: MqttBroker,
    pub external: Vec<MqttBroker>,
}

struct InnerConfig {
    database_url: String,
    brokers: Vec<MappedBroker>,
    plugin_dir: String,
    server_port: String,
    mqtt_client_id: String,
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

    pub fn mqtt_client_id(&self) -> &String {
        &self.inner.mqtt_client_id
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

impl From<Yaml> for MqttConnectionDetail {
    fn from(yaml: Yaml) -> Self {
        let scheme: MqttScheme = match yaml["scheme"]
            .as_str()
            .expect("mqtt.brokers.scheme must be set")
        {
            "tcp" => MqttScheme::Tcp,
            "wss" => MqttScheme::Wss,
            _ => panic!("mqtt.brokers.scheme must be specify tcp or wss scheme."),
        };
        let host = yaml["host"]
            .as_str()
            .expect("mqtt.brokers.internal|external.uri must be set")
            .to_owned();
        let port = yaml["port"]
            .as_i64()
            .expect("mqtt.brokers.port must be set") as u16;

        MqttConnectionDetail { scheme, port, host }
    }
}

impl From<Yaml> for BrokerCredentials {
    fn from(yaml: Yaml) -> Self {
        let username = yaml["username"]
            .as_str()
            .expect("mqtt.brokers.username must be set");
        let password = yaml["password"]
            .as_str()
            .expect("mqtt.brokers.password must be set");
        BrokerCredentials {
            username: username.to_owned(),
            password: password.to_owned(),
        }
    }
}

impl From<Yaml> for MqttBroker {
    fn from(yaml: Yaml) -> Self {
        let connection = yaml["connection"].clone().into();
        let credentials = yaml["credentials"].clone();
        let credentials = if credentials.is_badvalue() {
            if !cfg!(debug_assertions) {
                panic!("mqtt.brokers.credentials must be set in production mode");
            }
            None
        } else {
            Some(BrokerCredentials::from(credentials))
        };

        MqttBroker {
            connection,
            credentials,
        }
    }
}

impl From<Yaml> for MappedBroker {
    fn from(yaml: Yaml) -> Self {
        let internal = yaml["internal"].clone();
        let external = yaml["external"].clone();
        let broker = MappedBroker {
            internal: internal.into(),
            external: from_external_brokers(external),
        };

        if broker.external.is_empty() {
            panic!("mqtt.brokers.external cannot be empty");
        }
        broker
    }
}

fn from_external_brokers(yaml: Yaml) -> Vec<MqttBroker> {
    let credentials = yaml["credentials"].clone();
    let credentials = if credentials.is_badvalue() {
        if !cfg!(debug_assertions) {
            panic!("mqtt.brokers.credentials must be set in production mode");
        }
        None
    } else {
        Some(BrokerCredentials::from(credentials))
    };

    let connections = yaml["connections"]
        .as_vec()
        .expect("mqtt.external.connections is not set")
        .clone();

    connections
        .into_iter()
        .map(|c| MqttBroker {
            connection: c.into(),
            credentials: credentials.clone(),
        })
        .collect()
}

// Usage:
//let external_brokers: Vec<MqttBroker> = yaml["external"].into();

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

        let mqtt_client_id = yaml["mqtt"]["client_id"]
            .as_str()
            .map(|s| s.to_owned())
            .expect("mqtt.client_id must be set.");
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
                mqtt_client_id,
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
