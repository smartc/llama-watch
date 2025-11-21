pub mod handlers;

use axum::{
    routing::{get, post},
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
        .nest_service("/", ServeDir::new("static"))
        .with_state(state)
}
