#! /usr/bin/python3
import requests as re

BASE_URL = "http://127.0.0.1:8000/api"


def main():
    # Register sensor
    r = re.post(f'{BASE_URL}/sensor', json={})
    sensor_id = 1
    sensor_id = r.json()['id']
    sensor_key = 'ljo9OE85'
    sensor_key = r.json()['key']

    print(f"Registered sensor {sensor_id} {sensor_key}")

    r = re.post(f'{BASE_URL}/agent/{sensor_id}/{sensor_key}',
                json={"domain": "01_Licht", "agent_name": "TimeAgent"})
    r = re.post(f'{BASE_URL}/agent/{sensor_id}/{sensor_key}',
                json={"domain": "02_Wasser", "agent_name": "ThresholdAgent"})
    r = re.post(f'{BASE_URL}/agent/{sensor_id}/{sensor_key}',
                json={"domain": "03_Test", "agent_name": "TestAgent"})
    r = re.post(f'{BASE_URL}/agent/{sensor_id}/{sensor_key}',
                json={"domain": "03_wasm", "agent_name": "untouched"})


if __name__ == "__main__":
    main()
