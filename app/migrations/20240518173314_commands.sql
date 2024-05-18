-- Add migration script here
CREATE TABLE agent_commands (
    sensor_id INTEGER REFERENCES sensors(id) NOT NULL,
    domain VARCHAR(255) NOT NULL,
    command INT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP NOT NULL
);