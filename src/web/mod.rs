pub mod handlers;

use axum::{
    routing::{delete, get, post, put},
    Router,
};
use handlers::SharedWebState;
use tower_http::services::ServeDir;

pub fn create_router(state: SharedWebState) -> Router {
    Router::new()
        .route("/api/config", get(handlers::get_config).post(handlers::update_config))
        .route("/api/status", get(handlers::get_status))
        .route("/api/monitors/mqtt/:id/toggle", post(handlers::toggle_mqtt_monitor))
        .route("/api/monitors/alpaca/:id/toggle", post(handlers::toggle_alpaca_monitor))
        // Logging endpoints
        .route("/api/logging/status", get(handlers::get_logging_status))
        .route("/api/logging/toggle", post(handlers::toggle_logging))
        .route("/api/logging/files", get(handlers::list_log_files))
        .route("/api/logging/download", get(handlers::download_current_log))
        .route("/api/logging/download/:filename", get(handlers::download_log_file))
        // Weather device endpoints
        .route("/api/weather/devices", get(handlers::get_weather_devices).post(handlers::create_weather_device))
        .route("/api/weather/devices/:id", get(handlers::get_weather_device).put(handlers::update_weather_device).delete(handlers::delete_weather_device))
        .route("/api/weather/devices/:id/toggle", post(handlers::toggle_weather_device))
        .route("/api/weather/devices/:id/connect", post(handlers::set_weather_device_connection))
        .route("/api/weather/status", get(handlers::get_weather_status))
        // Alpaca discovery endpoints
        .route("/api/discovery/scan", get(handlers::discover_devices).post(handlers::discover_devices_with_options))
        .route("/api/discovery/probe", post(handlers::probe_device))
        // Monitor groups endpoints
        .route("/api/groups", get(handlers::get_monitor_groups).post(handlers::create_monitor_group))
        .route("/api/groups/status", get(handlers::get_monitor_group_statuses))
        .route("/api/groups/:id", get(handlers::get_monitor_group).put(handlers::update_monitor_group).delete(handlers::delete_monitor_group))
        .nest_service("/", ServeDir::new("static"))
        .with_state(state)
}
