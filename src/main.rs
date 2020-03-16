#[macro_use]
extern crate diesel;
#[macro_use]
extern crate slog;

extern crate actix;
extern crate actix_web;
extern crate dotenv;
extern crate futures;
extern crate rumq_client;
extern crate serde;
extern crate serde_json;
extern crate sloggers;
extern crate tokio;

mod agent;
mod logging;
pub mod models;
mod observer;
mod rest;
pub mod schema;
mod sensor;

#[actix_rt::main]
async fn main() {
    let (sensor_arc, dispatch_mqtt) = observer::ConcurrentSensorObserver::new();

    futures::future::join(rest::dispatch_server(sensor_arc.clone()), dispatch_mqtt).await;
}
