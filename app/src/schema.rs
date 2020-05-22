table! {
    agent_configs (sensor_id, domain) {
        sensor_id -> Int4,
        domain -> Text,
        agent_impl -> Text,
        state_json -> Text,
    }
}

table! {
    sensor_data (id) {
        id -> Int4,
        sensor_id -> Int4,
        timestamp -> Timestamp,
        battery -> Nullable<Float8>,
        moisture -> Nullable<Float8>,
        temperature -> Nullable<Float8>,
        carbon -> Nullable<Int4>,
        conductivity -> Nullable<Int4>,
        light -> Nullable<Int4>,
    }
}

table! {
    sensors (id) {
        id -> Int4,
        name -> Varchar,
    }
}

joinable!(agent_configs -> sensors (sensor_id));
joinable!(sensor_data -> sensors (sensor_id));

allow_tables_to_appear_in_same_query!(
    agent_configs,
    sensor_data,
    sensors,
);
