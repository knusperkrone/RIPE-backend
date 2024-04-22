#! /usr/bin/python3
import requests as re
import paho.mqtt.client as mqtt
import time as t

BASE_URL = "http://127.0.0.1:8000/api"
MQTT_URL = "wss01.knukro.com"
MQTT_PORT = 443


sensor_id = None
sensor_key = None


def main():
    global sensor_id, sensor_key
    if sensor_id is None or sensor_key is None:
        # Register sensor
        r = re.post(f"{BASE_URL}/sensor", json={})
        sensor_id = r.json()["id"]
        sensor_key = r.json()["key"]

        print(f"Registered sensor {sensor_id} {sensor_key}")
        # r = re.post(
        #     f"{BASE_URL}/agent/{sensor_id}",
        #     headers={"X-KEY": sensor_key},
        #     json={"domain": "01_Licht", "agent_name": "TimeAgent"},
        # )

        r = re.post(
            f"{BASE_URL}/agent/{sensor_id}",
            headers={"X-KEY": sensor_key},
            json={"domain": "02_Wasser", "agent_name": "ThresholdAgent"},
        )
        # print("TestAgent")
        # r = re.post(
        #     f"{BASE_URL}/agent/{sensor_id}",
        #     headers={"X-KEY": sensor_key},
        #     json={"domain": "03_Test", "agent_name": "TestAgent"},
        # )
        # print("PercentAgent")
        # r = re.post(
        #     f"{BASE_URL}/agent/{sensor_id}",
        #     headers={"X-KEY": sensor_key},
        #     json={"domain": "04_Lüftung", "agent_name": "PercentAgent"},
        # )
    else:
        print(f"Using sensor {sensor_id} {sensor_key}")

    def on_connect(client, userdata, flags, rc, _):
        print(f"Connected to {MQTT_URL} with result code " + str(rc))
        

    client = mqtt.Client(
        client_id="dev-client-id",
        reconnect_on_failure=False,
        protocol=mqtt.MQTTv5,
        transport="websockets",
    )
    client.tls_set()
    client.connect(
        MQTT_URL,
        MQTT_PORT,
        keepalive=30,
    )
    client.username_pw_set(
        "public", "j6ugLK1XUtGOYbecJ3zDfHlPZRL8FfkmgyvMbGZZfnLVBZ7eLKjpyqjMZxdETVISX"
    )
    client.on_connect = on_connect
    
    client.loop_start()
    while True:
        t.sleep(5)
        print(f"Sending data sensor/data/{sensor_id}/{sensor_key}")

        client.publish(
            f"sensor/data/{sensor_id}/{sensor_key}",
            payload='{"battery": 0, "moisture": 5.0, "light": 253, "temperature": 19.2, "conductivity": 0}',
        )



if __name__ == "__main__":
    main()
