use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
};
use parking_lot::RwLock;
use std::sync::Arc;

use crate::{
    config::{models::AppConfig, save_config},
    monitors::MonitorState,
};

pub type SharedConfig = Arc<RwLock<AppConfig>>;

pub struct WebState {
    pub config: SharedConfig,
    pub monitor_state: Arc<RwLock<MonitorState>>,
}

pub type SharedWebState = Arc<WebState>;

// GET /api/config - Get current configuration
pub async fn get_config(
    State(state): State<SharedWebState>,
) -> Json<AppConfig> {
    let config = state.config.read().clone();
    Json(config)
}

// POST /api/config - Update configuration
pub async fn update_config(
    State(state): State<SharedWebState>,
    Json(new_config): Json<AppConfig>,
) -> Result<StatusCode, (StatusCode, String)> {
    // Validate configuration
    if let Err(e) = new_config.validate() {
        return Err((StatusCode::BAD_REQUEST, e.to_string()));
    }

    // Save configuration
    if let Err(e) = save_config(&new_config).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }

    // Update in-memory configuration
    *state.config.write() = new_config;

    // Note: Monitors are not restarted automatically
    // The user needs to restart the application for changes to take effect

    Ok(StatusCode::OK)
}

// GET /api/status - Get all monitor statuses
pub async fn get_status(
    State(state): State<SharedWebState>,
) -> Json<serde_json::Value> {
    let monitor_state = state.monitor_state.read();
    let statuses = monitor_state.get_all_statuses();
    let is_safe = monitor_state.is_safe();

    Json(serde_json::json!({
        "is_safe": is_safe,
        "monitors": statuses,
    }))
}
