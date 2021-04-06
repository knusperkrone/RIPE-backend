use super::build_response;
use crate::sensor::ConcurrentSensorObserver;
use std::sync::Arc;
use warp::Filter;

pub fn routes(
    observer: &Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    get_active_agents(observer.clone())
        .or(register_agent(observer.clone()))
        .or(unregister_agent(observer.clone()))
        .or(on_agent_cmd(observer.clone()))
        .or(agent_config(observer.clone()))
        .or(set_agent_config(observer.clone()))
}

/// GET api/agent
///
/// Show all active agent plugins
///
/// Returns a string list of all names
fn get_active_agents(
    observer: Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::get())
        .and(warp::path!("api" / "agent"))
        .and_then(|observer: Arc<ConcurrentSensorObserver>| async move {
            let agents = observer.agents().await;
            build_response(Ok(agents))
        })
        .boxed()
}

/// POST api/agent/:id/:pwd
///
/// Register an agent to a sensor
///
/// Returns 200 if the new agent was added
fn register_agent(
    observer: Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::post())
        .and(warp::path!("api" / "agent" / i32 / String))
        .and(warp::body::json())
        .and_then(
            |observer: Arc<ConcurrentSensorObserver>,
             sensor_id: i32,
             key_b64: String,
             body: dto::AgentDto| async move {
                let domain = body.domain;
                let agent_name = body.agent_name;
                let resp = observer
                    .register_agent(sensor_id, key_b64, domain, agent_name)
                    .await;
                build_response(resp)
            },
        )
        .boxed()
}

/// DELETE api/agent/:id/:pwd
///
/// Unregister an agent to a sensor
///
/// Returns 200 if the new agent was removed
fn unregister_agent(
    observer: Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::delete())
        .and(warp::path!("api" / "agent" / i32 / String))
        .and(warp::body::json())
        .and_then(
            |observer: Arc<ConcurrentSensorObserver>,
             sensor_id: i32,
             key_b64: String,
             body: dto::AgentDto| async move {
                let domain = body.domain;
                let agent_name = body.agent_name;
                let resp = observer
                    .unregister_agent(sensor_id, key_b64, domain, agent_name)
                    .await;
                build_response(resp)
            },
        )
        .boxed()
}

/// POST api/agent/:id/:pwd/:domain
///
/// Sends an command to an agent
///
/// Returns 200 if the new agent was notified
fn on_agent_cmd(
    observer: Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::post())
        .and(warp::path!("api" / "agent" / i32 / String / String))
        .and(warp::body::json())
        .and_then(
            |observer: Arc<ConcurrentSensorObserver>,
             sensor_id: i32,
             key_b64: String,
             domain: String,
             body: dto::ForceRequest| async move {
                let resp = observer
                    .on_agent_cmd(sensor_id, key_b64, domain, body.payload)
                    .await;
                build_response(resp)
            },
        )
        .boxed()
}

/// GET api/agent/:id/:pwd/:domain/config
///
/// Get the config for an agent
///
/// Returns a HashMap with name and AgentConfigType
fn agent_config(
    observer: Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::get())
        .and(warp::path!(
            "api" / "agent" / i32 / String / String / "config"
        ))
        .and_then(
            |observer: Arc<ConcurrentSensorObserver>,
             sensor_id: i32,
             key_b64: String,
             domain: String| async move {
                let resp = observer.agent_config(sensor_id, key_b64, domain).await;
                build_response(resp)
            },
        )
        .boxed()
}

/// POST api/agent/:id/:pwd/:domain/config
///
/// Sets the config for an agent
///
/// Returns 200, if the HashMap was valid and accepted by the agent
fn set_agent_config(
    observer: Arc<ConcurrentSensorObserver>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || observer.clone())
        .and(warp::path!(
            "api" / "agent" / i32 / String / String / "config"
        ))
        .and(warp::body::json())
        .and(warp::post())
        .and_then(
            |observer: Arc<ConcurrentSensorObserver>,
             sensor_id: i32,
             key_b64: String,
             domain: String,
             body| async move {
                let resp = observer
                    .set_agent_config(sensor_id, key_b64, domain, body)
                    .await;
                build_response(resp)
            },
        )
        .boxed()
}

///
/// DTO
///
pub mod dto {
    use iftem_core::AgentUI;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug)]
    pub struct AgentDto {
        pub domain: String,
        pub agent_name: String,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct AgentStatusDto {
        pub domain: String,
        pub agent_name: String,
        pub ui: AgentUI,
    }

    #[derive(Serialize, Deserialize)]
    pub struct ForceRequest {
        pub payload: i64,
    }
}

///
/// TEST
///
#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use super::*;
    use chrono::Utc;
    use iftem_core::{AgentConfigType, AgentState, AgentUIDecorator, SliderFormatter};

    use crate::models::establish_db_connection;

    #[tokio::test]
    async fn test_print_serialized_agent_state() {
        println!(
            "Slider: {}",
            serde_json::json!(AgentUIDecorator::Slider(0.0, 1.0, 0.5))
        );
        println!(
            "TimePane: {}",
            serde_json::json!(AgentUIDecorator::TimePane(60))
        );

        println!(
            "Active: {}",
            serde_json::json!(AgentState::Executing(Utc::now()))
        );
        println!(
            "Forced: {}",
            serde_json::json!(AgentState::Forced(Utc::now()))
        );
        println!("Ready: {}", serde_json::json!(AgentState::Ready));
        println!("Default: {}", serde_json::json!(AgentState::Disabled));
        println!("Error: {}", serde_json::json!(AgentState::Error));
    }

    #[tokio::test]
    async fn test_print_serialized_agent_config() {
        println!(
            "Swich: {}",
            serde_json::json!(AgentConfigType::Switch(true))
        );
        println!(
            "Datetime: {}",
            serde_json::json!(AgentConfigType::DateTime(0))
        );
        println!(
            "IntRange: {}",
            serde_json::json!(AgentConfigType::IntRange(0, 10, 5))
        );
        println!(
            "IntSliderRange: {}",
            serde_json::json!(AgentConfigType::IntSliderRange(
                SliderFormatter::Linear,
                0,
                10,
                5
            ))
        );
        println!(
            "FloatRange: {}",
            serde_json::json!(AgentConfigType::FloatRange(0., 10., 5.))
        );
        println!(
            "FloatSliderRange: {}",
            serde_json::json!(AgentConfigType::FloatSliderRange(
                SliderFormatter::Linear,
                0.0,
                10.0,
                5.0
            ))
        );
    }

    #[tokio::test]
    async fn test_rest_agents() {
        // Prepare
        let db_conn = establish_db_connection();
        let observer = ConcurrentSensorObserver::new(db_conn);
        let routes = routes(&observer);

        // Execute
        let res = warp::test::request()
            .path("/api/agent")
            .reply(&routes)
            .await;

        // Validate
        assert_eq!(200, res.status());
        let _: Vec<String> = serde_json::from_slice(res.body()).unwrap();
    }

    async fn handle_rejection(
        err: warp::Rejection,
    ) -> Result<impl warp::Reply, std::convert::Infallible> {
        println!("ERR: {:?}", err);
        Ok(warp::reply::with_status(
            "",
            warp::http::StatusCode::BAD_REQUEST,
        ))
    }

    #[tokio::test]
    async fn test_rest_register_agent() {
        // Prepare
        let db_conn = establish_db_connection();
        let observer = ConcurrentSensorObserver::new(db_conn);
        let sensor = observer.register_sensor(None).await.unwrap();
        let routes = routes(&observer).recover(handle_rejection);

        // Execute
        let dto = dto::AgentDto {
            agent_name: "MockAgent".to_owned(),
            domain: "Test".to_owned(),
        };
        let res = warp::test::request()
            .method("POST")
            .path(&format!("/api/agent/{}/{}", sensor.id, sensor.key))
            .json(&dto)
            .reply(&routes)
            .await;

        // Validate
        assert_eq!(200, res.status());
    }

    #[tokio::test]
    async fn test_rest_unregister_agent() {
        // Prepare
        let agent_name = "MockAgent".to_owned();
        let domain = "Test".to_owned();
        let db_conn = establish_db_connection();
        let observer = ConcurrentSensorObserver::new(db_conn);
        let sensor = observer.register_sensor(None).await.unwrap();
        observer
            .register_agent(
                sensor.id,
                sensor.key.clone(),
                domain.clone(),
                agent_name.clone(),
            )
            .await
            .unwrap();
        let routes = routes(&observer).recover(handle_rejection);

        // Execute
        let dto = dto::AgentDto { domain, agent_name };
        let res = warp::test::request()
            .method("DELETE")
            .path(&format!("/api/agent/{}/{}", sensor.id, sensor.key))
            .json(&dto)
            .reply(&routes)
            .await;

        // Validate
        assert_eq!(200, res.status());
    }

    #[tokio::test]
    async fn test_rest_agent_cmd() {
        // Prepare
        let agent_name = "MockAgent".to_owned();
        let domain = "Test".to_owned();
        let db_conn = establish_db_connection();
        let observer = ConcurrentSensorObserver::new(db_conn);
        let sensor = observer.register_sensor(None).await.unwrap();
        observer
            .register_agent(
                sensor.id,
                sensor.key.clone(),
                domain.clone(),
                agent_name.clone(),
            )
            .await
            .unwrap();
        let routes = routes(&observer).recover(handle_rejection);

        // Execute
        let dto = dto::ForceRequest { payload: 1 };
        let res = warp::test::request()
            .method("POST")
            .path(&format!(
                "/api/agent/{}/{}/{}",
                sensor.id, sensor.key, domain
            ))
            .json(&dto)
            .reply(&routes)
            .await;

        // Validate
        assert_eq!(200, res.status());
    }

    #[tokio::test]
    async fn test_rest_agent_config() {
        // Prepare
        let agent_name = "MockAgent".to_owned();
        let domain = "Test".to_owned();
        let db_conn = establish_db_connection();
        let observer = ConcurrentSensorObserver::new(db_conn);
        let sensor = observer.register_sensor(None).await.unwrap();
        observer
            .register_agent(
                sensor.id,
                sensor.key.clone(),
                domain.clone(),
                agent_name.clone(),
            )
            .await
            .unwrap();
        let routes = routes(&observer).recover(handle_rejection);

        // Execute
        let dto = dto::ForceRequest { payload: 1 };
        let res = warp::test::request()
            .path(&format!(
                "/api/agent/{}/{}/{}/config",
                sensor.id, sensor.key, domain
            ))
            .json(&dto)
            .reply(&routes)
            .await;

        // Validate
        assert_eq!(200, res.status());
    }

    #[tokio::test]
    async fn test_rest_set_agent_config() {
        // Prepare
        let agent_name = "MockAgent".to_owned();
        let domain = "Test".to_owned();
        let db_conn = establish_db_connection();
        let observer = ConcurrentSensorObserver::new(db_conn);
        let sensor = observer.register_sensor(None).await.unwrap();
        observer
            .register_agent(
                sensor.id,
                sensor.key.clone(),
                domain.clone(),
                agent_name.clone(),
            )
            .await
            .unwrap();
        let routes = routes(&observer).recover(handle_rejection);

        // Execute
        let dto = HashMap::<String, String>::new();
        let res = warp::test::request()
            .method("POST")
            .path(&format!(
                "/api/agent/{}/{}/{}/config",
                sensor.id, sensor.key, domain
            ))
            .json(&dto)
            .reply(&routes)
            .await;

        // Validate
        assert_eq!(200, res.status());
    }
}
