use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;

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
    let config = state.config.read().await.clone();
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

    // Reload monitors with new configuration
    let mut monitor_state = state.monitor_state.write().await;
    if let Err(e) = monitor_state.reload(&new_config).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to reload monitors: {}", e)));
    }
    drop(monitor_state); // Release lock

    // Update in-memory configuration
    *state.config.write().await = new_config;

    Ok(StatusCode::OK)
}

// GET /api/status - Get all monitor statuses
pub async fn get_status(
    State(state): State<SharedWebState>,
) -> Json<serde_json::Value> {
    let monitor_state = state.monitor_state.read().await;
    let config = state.config.read().await;

    let mut statuses = monitor_state.get_all_statuses();
    let is_safe = monitor_state.is_safe();

    // Add disabled monitors from config (they won't be in active monitor state)
    let mut active_ids: std::collections::HashSet<String> = statuses
        .iter()
        .map(|s| s.id.clone())
        .collect();

    // Add disabled MQTT monitors
    for (id, mqtt_config) in &config.mqtt_monitors {
        if !mqtt_config.enabled && !active_ids.contains(id) {
            statuses.push(crate::config::models::MonitorStatus {
                id: id.clone(),
                name: mqtt_config.name.clone(),
                monitor_type: "MQTT".to_string(),
                is_safe: false,
                current_value: None,
                threshold: mqtt_config.threshold,
                last_update: None,
                error: Some("Monitor disabled".to_string()),
                raw_payload: None,
                enabled: false,
            });
            active_ids.insert(id.clone());
        }
    }

    // Add disabled ALPACA monitors
    for (id, alpaca_config) in &config.alpaca_monitors {
        if !alpaca_config.enabled && !active_ids.contains(id) {
            statuses.push(crate::config::models::MonitorStatus {
                id: id.clone(),
                name: alpaca_config.name.clone(),
                monitor_type: "ALPACA".to_string(),
                is_safe: false,
                current_value: None,
                threshold: alpaca_config.threshold,
                last_update: None,
                error: Some("Monitor disabled".to_string()),
                raw_payload: None,
                enabled: false,
            });
        }
    }

    Json(serde_json::json!({
        "is_safe": is_safe,
        "monitors": statuses,
    }))
}

#[derive(Deserialize)]
pub struct ToggleEnabledRequest {
    enabled: bool,
}

// POST /api/monitors/mqtt/:id/toggle - Toggle MQTT monitor enabled state
pub async fn toggle_mqtt_monitor(
    Path(id): Path<String>,
    State(state): State<SharedWebState>,
    Json(req): Json<ToggleEnabledRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut config = state.config.write().await;

    // Find and update the monitor
    if let Some(monitor) = config.mqtt_monitors.get_mut(&id) {
        monitor.enabled = req.enabled;
    } else {
        return Err((StatusCode::NOT_FOUND, format!("Monitor '{}' not found", id)));
    }

    // Save configuration
    if let Err(e) = save_config(&config).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }

    // Reload monitors
    let mut monitor_state = state.monitor_state.write().await;
    if let Err(e) = monitor_state.reload(&config).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to reload monitors: {}", e)));
    }

    Ok(StatusCode::OK)
}

// POST /api/monitors/alpaca/:id/toggle - Toggle Alpaca monitor enabled state
pub async fn toggle_alpaca_monitor(
    Path(id): Path<String>,
    State(state): State<SharedWebState>,
    Json(req): Json<ToggleEnabledRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut config = state.config.write().await;

    // Find and update the monitor
    if let Some(monitor) = config.alpaca_monitors.get_mut(&id) {
        monitor.enabled = req.enabled;
    } else {
        return Err((StatusCode::NOT_FOUND, format!("Monitor '{}' not found", id)));
    }

    // Save configuration
    if let Err(e) = save_config(&config).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }

    // Reload monitors
    let mut monitor_state = state.monitor_state.write().await;
    if let Err(e) = monitor_state.reload(&config).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to reload monitors: {}", e)));
    }

    Ok(StatusCode::OK)
}
