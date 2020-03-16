CREATE TABLE sensors
(
    id SERIAL PRIMARY KEY,
    name VARCHAR (255) NOT NULL
);

CREATE TABLE agent_configs
(
    sensor_id INTEGER REFERENCES sensors(id),
    domain TEXT NOT NULL,
    agent_impl TEXT NOT NULL,
    state_json TEXT NOT NULL,
    PRIMARY KEY(sensor_id, domain)
);
