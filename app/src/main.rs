#[macro_use]
extern crate diesel;
#[macro_use]
extern crate slog;

mod agent;
mod error;
mod logging;
mod models;
mod observer;
mod rest;
mod schema;

#[actix_rt::main]
async fn main() {
    let sensor_arc = observer::ConcurrentSensorObserver::new();
    let dispatch_mqtt = observer::ConcurrentSensorObserver::dispatch_mqtt(sensor_arc.clone());
    let dispatch_ipc = observer::ConcurrentSensorObserver::dispatch_ipc(sensor_arc.clone());
    let dispatch_rest = rest::dispatch_server(sensor_arc.clone());

    tokio::join!(dispatch_mqtt, dispatch_ipc, dispatch_rest);
}
