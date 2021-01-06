#! /usr/bin/python3
import requests as re

BASE_URL = "http://127.0.0.1:8000/api"


def main():
    # Register sensor
    r = re.post(f'{BASE_URL}/sensor', json={})
    sensor_id = r.json()['id']
    sensor_key = r.json()['key']

    print(f"Registered sensor {sensor_id} {sensor_key}")

    r = re.post(f'{BASE_URL}/agent/{sensor_id}/{sensor_key}',
                json={"domain": "01_TEST", "agent_name": "TestAgent"})
    print("Added TestAgent")
    r = re.post(f'{BASE_URL}/agent/{sensor_id}/{sensor_key}',
                json={"domain": "02_Wasser", "agent_name": "ThresholdAgent"})
    print("Added ThresholdAgent")
    r = re.post(f'{BASE_URL}/agent/{sensor_id}/{sensor_key}',
                json={"domain": "03_Licht", "agent_name": "TimeAgent"})
    print("Added TimeAgent")


if __name__ == "__main__":
    main()
