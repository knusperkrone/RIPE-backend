#! /usr/bin/python3
import requests as re

BASE_URL = "http://127.0.0.1:8000/api"


def main():
    # Register sensor
    r = re.post(f'{BASE_URL}/sensor', json={})
    sensor_id = r.json()['id']
    sensor_key = r.json()['key']

    print(f"Registered sensor {sensor_id} {sensor_key}")
    r = re.post(f'{BASE_URL}/agent/{sensor_id}', headers={'X-KEY': sensor_key},
                json={"domain": "01_Licht", "agent_name": "TimeAgent"})
    print("TimeAgent")
    r = re.post(f'{BASE_URL}/agent/{sensor_id}', headers={'X-KEY': sensor_key},
                json={"domain": "02_Wasser", "agent_name": "ThresholdAgent"})
    print("TestAgent")
    r = re.post(f'{BASE_URL}/agent/{sensor_id}', headers={'X-KEY': sensor_key},
                json={"domain": "03_Test", "agent_name": "TestAgent"})
    print("PercentAgent")
    r = re.post(f'{BASE_URL}/agent/{sensor_id}', headers={'X-KEY': sensor_key},
                json={"domain": "04_Lüftung", "agent_name": "PercentAgent"})

if __name__ == "__main__":
    main()
