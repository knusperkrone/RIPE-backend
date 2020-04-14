table! {
    agent_configs (sensor_id, domain) {
        sensor_id -> Int4,
        domain -> Text,
        agent_impl -> Text,
        state_json -> Text,
    }
}

table! {
    sensors (id) {
        id -> Int4,
        name -> Varchar,
    }
}

joinable!(agent_configs -> sensors (sensor_id));

allow_tables_to_appear_in_same_query!(
    agent_configs,
    sensors,
);
