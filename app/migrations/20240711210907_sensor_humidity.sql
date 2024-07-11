-- Add migration script here
ALTER TABLE sensor_data
ADD COLUMN humidity FLOAT;