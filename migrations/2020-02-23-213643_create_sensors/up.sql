CREATE TABLE sensors
(
    id SERIAL PRIMARY KEY,
    key_b64 VARCHAR (6) NOT NULL,
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

CREATE TABLE sensor_data
(
    id SERIAL PRIMARY KEY,
    sensor_id INTEGER REFERENCES sensors NOT NULL,
    timestamp TIMESTAMP NOT NULL,
    battery FLOAT,
    moisture FLOAT,
    temperature FLOAT,
    carbon INT,
    conductivity INT,
    light INT
);
/*

*/