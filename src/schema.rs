table! {
    sensor_agent_config (sensor_id, action) {
        sensor_id -> Int4,
        action -> Text,
        agent_impl -> Text,
        config_json -> Text,
    }
}

table! {
    sensors (id) {
        id -> Int4,
        name -> Text,
    }
}

joinable!(sensor_agent_config -> sensors (sensor_id));

allow_tables_to_appear_in_same_query!(
    sensor_agent_config,
    sensors,
);
