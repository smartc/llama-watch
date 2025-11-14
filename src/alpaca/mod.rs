pub mod models;
pub mod safety_monitor;

use axum::{
    routing::{get, put},
    Router,
};
use safety_monitor::SharedAppState;

pub fn create_router(state: SharedAppState) -> Router {
    Router::new()
        .route(
            "/api/v1/safetymonitor/:device/issafe",
            get(safety_monitor::is_safe),
        )
        .route(
            "/api/v1/safetymonitor/:device/connected",
            get(safety_monitor::get_connected).put(safety_monitor::put_connected),
        )
        .route(
            "/api/v1/safetymonitor/:device/name",
            get(safety_monitor::get_name),
        )
        .route(
            "/api/v1/safetymonitor/:device/description",
            get(safety_monitor::get_description),
        )
        .route(
            "/api/v1/safetymonitor/:device/driverinfo",
            get(safety_monitor::get_driver_info),
        )
        .route(
            "/api/v1/safetymonitor/:device/driverversion",
            get(safety_monitor::get_driver_version),
        )
        .route(
            "/api/v1/safetymonitor/:device/interfaceversion",
            get(safety_monitor::get_interface_version),
        )
        .route(
            "/api/v1/safetymonitor/:device/supportedactions",
            get(safety_monitor::get_supported_actions),
        )
        .with_state(state)
}
