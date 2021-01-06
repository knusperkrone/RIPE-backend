use super::build_response;
use crate::sensor::ConcurrentSensorObserver;
use actix_web::{web, HttpResponse};
use iftem_core::AgentConfigType;
use std::{collections::HashMap, sync::Arc};

#[derive(serde::Deserialize)]
struct ForceRequest {
    payload: i64,
}

/// Registers an agent to a sensor
///
/// Returns 200 if the new agent was added
async fn agent_register(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    register_request: web::Json<dto::AgentRegisterDto>,
    path: web::Path<(i32, String)>,
) -> HttpResponse {
    let (sensor_id, key_b64) = path.into_inner();
    let req = register_request.0;
    let resp = observer.register_agent(sensor_id, key_b64, req).await;
    build_response(resp)
}

/// Unregisters an agent to a sensor
///
/// Returns 200 if the new agent was removed
async fn agent_unregister(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    register_request: web::Json<dto::AgentRegisterDto>,
    path: web::Path<(i32, String)>,
) -> HttpResponse {
    let (sensor_id, key_b64) = path.into_inner();
    let req = register_request.0;
    let resp = observer.unregister_agent(sensor_id, key_b64, req).await;
    build_response(resp)
}

async fn on_agent_cmd(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    path: web::Path<(i32, String, String)>,
    params: web::Json<ForceRequest>,
) -> HttpResponse {
    let (sensor_id, key_b64, domain) = path.into_inner();
    let payload = params.payload;
    let resp = observer
        .on_agent_cmd(sensor_id, key_b64, domain, payload)
        .await;
    build_response(resp)
}

async fn agent_config(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    path: web::Path<(i32, String, String)>,
) -> HttpResponse {
    let (sensor_id, key_b64, domain) = path.into_inner();
    let resp = observer.agent_config(sensor_id, key_b64, domain).await;
    build_response(resp)
}

async fn on_agent_config(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    path: web::Path<(i32, String, String)>,
    config_request: web::Json<HashMap<String, AgentConfigType>>,
) -> HttpResponse {
    let (sensor_id, key_b64, domain) = path.into_inner();
    let resp = observer
        .on_agent_config(sensor_id, key_b64, domain, config_request.0)
        .await;
    build_response(resp)
}

/// Show all active agent plugins
///
/// Returns a string list of all names
async fn get_active_agents(observer: web::Data<Arc<ConcurrentSensorObserver>>) -> HttpResponse {
    let agents = observer.agents().await;
    HttpResponse::Ok().json(agents)
}

/// Config
///
/// Registers endpoints of this file
pub fn config_endpoints(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("api/agent").route(web::get().to(get_active_agents)))
        .service(
            web::resource("api/agent/{id}/{key}")
                .route(web::post().to(agent_register))
                .route(web::delete().to(agent_unregister)),
        )
        .service(
            web::resource("api/agent/{id}/{key}/{domain}/config")
                .route(web::get().to(agent_config))
                .route(web::post().to(on_agent_config)),
        )
        .service(
            web::resource("api/agent/{id}/{key}/{domain}").route(web::post().to(on_agent_cmd)),
        );
}

///
/// DTO
/// 
pub mod dto {
    use iftem_core::AgentUI;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug)]
    pub struct AgentRegisterDto {
        pub domain: String,
        pub agent_name: String,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct AgentStatusDto {
        pub domain: String,
        pub agent_name: String,
        pub ui: AgentUI,
    }
}

///
/// TEST
///
#[cfg(test)]
mod test {
    use super::*;
    use crate::rest::AgentRegisterDto;
    use actix_web::dev::Service;
    use actix_web::{test, web, App};
    use iftem_core::{AgentState, AgentUIDecorator};

    #[actix_rt::test]
    async fn test_print_serialized_agent_state() {
        AgentUIDecorator::Slider(0.0, 1.0, 0.5);
        AgentUIDecorator::TimePane(60);

        println!(
            "Slider: {}",
            serde_json::json!(AgentUIDecorator::Slider(0.0, 1.0, 0.5))
        );
        println!(
            "TimePane: {}",
            serde_json::json!(AgentUIDecorator::TimePane(60))
        );
        println!("Active: {}", serde_json::json!(AgentState::Active));
        println!("Default: {}", serde_json::json!(AgentState::Disabled));
        println!("Forced: {}", serde_json::json!(AgentState::Error));
    }

    #[actix_rt::test]
    async fn test_register_agent() {
        // prepare
        let sensor_name = "Agent_Register_Sensor";
        let agent_domain = "Water";
        let mock_agent = "MockAgent";
        let observer = ConcurrentSensorObserver::new();
        let register = observer
            .register_new_sensor(&Some(sensor_name.to_owned()))
            .await
            .unwrap();
        let request_json = AgentRegisterDto {
            agent_name: mock_agent.to_owned(),
            domain: agent_domain.to_owned(),
        };

        let mut app = test::init_service(
            App::new()
                .app_data(web::Data::new(observer))
                .configure(super::config_endpoints),
        )
        .await;

        // Execute
        let req = test::TestRequest::post()
            .uri(&format!("/api/agent/{}/{}", register.id, register.key))
            .set_json(&request_json)
            .to_request();
        let resp = app.call(req).await.unwrap();

        // Validate
        assert_eq!(200, resp.status());
    }

    #[actix_rt::test]
    async fn test_unregister_agent() {
        // Prepare
        let sensor_name = "Agent_Unregister_Sensor";
        let agent_domain = "Water";
        let mock_agent = "MockAgent";
        let observer = ConcurrentSensorObserver::new();
        let register = observer
            .register_new_sensor(&Some(sensor_name.to_owned()))
            .await
            .unwrap();
        let register_request = AgentRegisterDto {
            agent_name: mock_agent.to_owned(),
            domain: agent_domain.to_owned(),
        };
        let request_json = AgentRegisterDto {
            agent_name: mock_agent.to_owned(),
            domain: agent_domain.to_owned(),
        };
        observer
            .register_agent(register.id, register.key.clone(), register_request)
            .await
            .unwrap();

        let mut app = test::init_service(
            App::new()
                .app_data(web::Data::new(observer))
                .configure(super::config_endpoints),
        )
        .await;

        // Execute
        let req = test::TestRequest::delete()
            .uri(&format!("/api/agent/{}/{}", register.id, register.key))
            .set_json(&request_json)
            .to_request();
        let resp = app.call(req).await.unwrap();

        // Validate
        assert_eq!(200, resp.status());
    }
}
