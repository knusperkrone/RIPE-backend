-- Add migration script here
CREATE TABLE sensor_log
(
    id SERIAL PRIMARY KEY,
    sensor_id INTEGER REFERENCES sensors(id) NOT NULL,
    time TIMESTAMP NOT NULL,
    log VARCHAR (255) NOT NULL
);