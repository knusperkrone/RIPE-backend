use super::build_response;
use crate::sensor::ConcurrentSensorObserver;
use actix_web::{web, HttpResponse};
use std::sync::Arc;

#[derive(serde::Deserialize)]
struct ForceRequest {
    active: bool,
    secs: u32,
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

async fn force_state(
    observer: web::Data<Arc<ConcurrentSensorObserver>>,
    path: web::Path<(i32, String, String)>,
    params: web::Query<ForceRequest>,
) -> HttpResponse {
    let (sensor_id, key_b64, domain) = path.into_inner();
    let active = params.active;
    let secs = chrono::Duration::seconds(params.secs as i64);
    let resp = observer
        .force_agent(sensor_id, key_b64, domain, active, secs)
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

pub fn config_endpoints(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("api/agent").route(web::get().to(get_active_agents)))
        .service(
            web::resource("api/agent/{id}/{key}")
                .route(web::post().to(agent_register))
                .route(web::delete().to(agent_unregister)),
        )
        .service(web::resource("api/agent/{id}/{key}/{domain}").route(web::post().to(force_state)));
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
    use chrono::Utc;
    use iftem_core::{AgentState, AgentUIDecorator};

    #[actix_rt::test]
    async fn test_print_agent_state() {
        AgentUIDecorator::Slider(0.0, 1.0);
        AgentUIDecorator::TimePane(60);

        println!(
            "Slider: {}",
            serde_json::json!(AgentUIDecorator::Slider(0.0, 1.0))
        );
        println!(
            "TimePane: {}",
            serde_json::json!(AgentUIDecorator::TimePane(60))
        );
        println!("Active: {}", serde_json::json!(AgentState::Active));
        println!("Default: {}", serde_json::json!(AgentState::Default));
        println!(
            "Forced: {}",
            serde_json::json!(AgentState::Forced(true, Utc::now()))
        );
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
