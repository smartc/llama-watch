pub mod models;
pub mod safety_monitor;
pub mod management;
pub mod discovery;
pub mod observing_conditions;
pub mod case_insensitive;

use axum::{
    routing::{get, put},
    response::Redirect,
    Router,
};
use safety_monitor::SharedAppState;
use observing_conditions::SharedObservingConditionsState;
pub use management::SharedManagementState;

/// Redirect to main web UI status tab
async fn setup_main() -> Redirect {
    Redirect::to("/#status")
}

/// Redirect to device setup (settings tab)
async fn setup_device() -> Redirect {
    Redirect::to("/#settings")
}

pub fn create_management_router(state: SharedManagementState) -> Router {
    Router::new()
        // Setup pages (redirects to web UI)
        .route("/setup", get(setup_main))
        // API versions endpoint (root level for ASCOM discovery)
        .route(
            "/api/apiversions",
            get(management::get_api_versions),
        )
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
        .with_state(state)
}

pub fn create_router(state: SharedAppState) -> Router {
    Router::new()
        // Setup pages (redirects to web UI)
        .route("/setup/v1/safetymonitor/:device/setup", get(setup_device))
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

pub fn create_observingconditions_router(state: SharedObservingConditionsState) -> Router {
    Router::new()
        // Setup pages (redirects to web UI)
        .route("/setup/v1/observingconditions/:device/setup", get(setup_device))
        // ObservingConditions-specific properties
        .route(
            "/api/v1/observingconditions/:device/cloudcover",
            get(observing_conditions::get_cloudcover),
        )
        .route(
            "/api/v1/observingconditions/:device/dewpoint",
            get(observing_conditions::get_dewpoint),
        )
        .route(
            "/api/v1/observingconditions/:device/humidity",
            get(observing_conditions::get_humidity),
        )
        .route(
            "/api/v1/observingconditions/:device/pressure",
            get(observing_conditions::get_pressure),
        )
        .route(
            "/api/v1/observingconditions/:device/rainrate",
            get(observing_conditions::get_rainrate),
        )
        .route(
            "/api/v1/observingconditions/:device/skybrightness",
            get(observing_conditions::get_skybrightness),
        )
        .route(
            "/api/v1/observingconditions/:device/skyquality",
            get(observing_conditions::get_skyquality),
        )
        .route(
            "/api/v1/observingconditions/:device/skytemperature",
            get(observing_conditions::get_skytemperature),
        )
        .route(
            "/api/v1/observingconditions/:device/starfwhm",
            get(observing_conditions::get_starfwhm),
        )
        .route(
            "/api/v1/observingconditions/:device/temperature",
            get(observing_conditions::get_temperature),
        )
        .route(
            "/api/v1/observingconditions/:device/winddirection",
            get(observing_conditions::get_winddirection),
        )
        .route(
            "/api/v1/observingconditions/:device/windgust",
            get(observing_conditions::get_windgust),
        )
        .route(
            "/api/v1/observingconditions/:device/windspeed",
            get(observing_conditions::get_windspeed),
        )
        // ObservingConditions methods
        .route(
            "/api/v1/observingconditions/:device/averageperiod",
            get(observing_conditions::get_averageperiod).put(observing_conditions::put_averageperiod),
        )
        .route(
            "/api/v1/observingconditions/:device/sensordescription",
            get(observing_conditions::get_sensordescription),
        )
        .route(
            "/api/v1/observingconditions/:device/timesincelastupdate",
            get(observing_conditions::get_timesincelastupdate),
        )
        .route(
            "/api/v1/observingconditions/:device/refresh",
            put(observing_conditions::put_refresh),
        )
        // Common properties
        .route(
            "/api/v1/observingconditions/:device/connected",
            get(observing_conditions::get_connected).put(observing_conditions::put_connected),
        )
        .route(
            "/api/v1/observingconditions/:device/name",
            get(observing_conditions::get_name),
        )
        .route(
            "/api/v1/observingconditions/:device/description",
            get(observing_conditions::get_description),
        )
        .route(
            "/api/v1/observingconditions/:device/driverinfo",
            get(observing_conditions::get_driverinfo),
        )
        .route(
            "/api/v1/observingconditions/:device/driverversion",
            get(observing_conditions::get_driverversion),
        )
        .route(
            "/api/v1/observingconditions/:device/interfaceversion",
            get(observing_conditions::get_interfaceversion),
        )
        .route(
            "/api/v1/observingconditions/:device/supportedactions",
            get(observing_conditions::get_supportedactions),
        )
        .with_state(state)
}
