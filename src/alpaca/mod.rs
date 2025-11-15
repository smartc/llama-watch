pub mod models;
pub mod safety_monitor;
pub mod management;

use axum::{
    routing::{get, put},
    Router,
};
use safety_monitor::SharedAppState;

pub fn create_router(state: SharedAppState) -> Router {
    Router::new()
        // Management API endpoints
        .route(
            "/management/apiversions",
            get(management::get_api_versions),
        )
        .route(
            "/management/v1/description",
            get(management::get_description),
        )
        .route(
            "/management/v1/configureddevices",
            get(management::get_configured_devices),
        )
        // SafetyMonitor specific
        .route(
            "/api/v1/safetymonitor/:device/issafe",
            get(safety_monitor::is_safe),
        )
        // Common properties
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
        // Common methods
        .route(
            "/api/v1/safetymonitor/:device/action",
            put(safety_monitor::put_action),
        )
        .route(
            "/api/v1/safetymonitor/:device/commandblind",
            put(safety_monitor::put_command_blind),
        )
        .route(
            "/api/v1/safetymonitor/:device/commandbool",
            put(safety_monitor::put_command_bool),
        )
        .route(
            "/api/v1/safetymonitor/:device/commandstring",
            put(safety_monitor::put_command_string),
        )
        .with_state(state)
}
