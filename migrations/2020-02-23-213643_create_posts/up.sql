-- Your SQL goes here

CREATE TABLE sensors (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE sensor_agent_config (
    sensor_id INTEGER REFERENCES sensors(id),
    action TEXT NOT NULL,
    agent_impl TEXT NOT NULL,
    config_json TEXT NOT NULL,
    PRIMARY KEY(sensor_id, action)
);
