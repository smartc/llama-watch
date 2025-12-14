use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, StatusCode},
    response::{Json, Response},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::io::ReaderStream;

use crate::{
    alpaca::safety_monitor::SafetyMonitor,
    config::{models::AppConfig, save_config},
    logging::{DebugLogger, LogFileInfo},
    monitors::MonitorState,
    weather::{WeatherMonitorManager, ObservingConditionsDevice},
};
use std::collections::HashMap;

pub type SharedConfig = Arc<RwLock<AppConfig>>;

pub struct WebState {
    pub config: SharedConfig,
    pub monitor_state: Arc<RwLock<MonitorState>>,
    pub debug_logger: Arc<DebugLogger>,
    pub safety_monitor: Arc<SafetyMonitor>,
    pub oc_devices: Arc<RwLock<HashMap<u32, Arc<ObservingConditionsDevice>>>>,
    pub weather_monitors: Arc<RwLock<WeatherMonitorManager>>,
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

    // Get ASCOM connected status
    let is_connected = state.safety_monitor.is_connected(&state.monitor_state).await;

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
                hold_time_seconds: mqtt_config.hold_time_seconds,
                pending_is_safe: None,
                pending_since: None,
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
                hold_time_seconds: alpaca_config.hold_time_seconds,
                pending_is_safe: None,
                pending_since: None,
            });
        }
    }

    // Sort monitors alphabetically by name for consistent display order
    statuses.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    Json(serde_json::json!({
        "is_safe": is_safe,
        "is_connected": is_connected,
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

// Logging API endpoints

#[derive(Serialize)]
pub struct LoggingStatusResponse {
    pub enabled: bool,
    pub current_file: String,
    pub files: Vec<LogFileInfo>,
}

// GET /api/logging/status - Get logging status
pub async fn get_logging_status(
    State(state): State<SharedWebState>,
) -> Json<LoggingStatusResponse> {
    let enabled = state.debug_logger.is_enabled();
    let current_file = state.debug_logger.get_current_log_path()
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let files = state.debug_logger.get_log_files().unwrap_or_default();

    Json(LoggingStatusResponse {
        enabled,
        current_file,
        files,
    })
}

#[derive(Deserialize)]
pub struct ToggleLoggingRequest {
    enabled: bool,
}

// POST /api/logging/toggle - Enable/disable logging
pub async fn toggle_logging(
    State(state): State<SharedWebState>,
    Json(req): Json<ToggleLoggingRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    // Update logger state
    if let Err(e) = state.debug_logger.set_enabled(req.enabled) {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to toggle logging: {}", e)));
    }

    // Update config
    let mut config = state.config.write().await;
    config.logging_enabled = req.enabled;

    // Save configuration
    if let Err(e) = save_config(&config).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }

    Ok(StatusCode::OK)
}

// GET /api/logging/files - List available log files
pub async fn list_log_files(
    State(state): State<SharedWebState>,
) -> Result<Json<Vec<LogFileInfo>>, (StatusCode, String)> {
    match state.debug_logger.get_log_files() {
        Ok(files) => Ok(Json(files)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to list log files: {}", e))),
    }
}

// GET /api/logging/download/:filename - Download a specific log file
pub async fn download_log_file(
    Path(filename): Path<String>,
    State(state): State<SharedWebState>,
) -> Result<Response<Body>, (StatusCode, String)> {
    // Get the log file path
    let path = match state.debug_logger.get_log_file_path(&filename) {
        Some(p) => p,
        None => return Err((StatusCode::NOT_FOUND, "Log file not found".to_string())),
    };

    // Open the file
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to open log file: {}", e))),
    };

    // Get file metadata for content length
    let metadata = match file.metadata().await {
        Ok(m) => m,
        Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to get file metadata: {}", e))),
    };

    // Create response with file stream
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::CONTENT_LENGTH, metadata.len())
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(body)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to build response: {}", e)))?;

    Ok(response)
}

// GET /api/logging/download - Download the current log file
pub async fn download_current_log(
    State(state): State<SharedWebState>,
) -> Result<Response<Body>, (StatusCode, String)> {
    let path = state.debug_logger.get_current_log_path();
    let filename = path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    // Check if file exists
    if !path.exists() {
        return Err((StatusCode::NOT_FOUND, "Current log file does not exist yet".to_string()));
    }

    // Open the file
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to open log file: {}", e))),
    };

    // Get file metadata for content length
    let metadata = match file.metadata().await {
        Ok(m) => m,
        Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to get file metadata: {}", e))),
    };

    // Create response with file stream
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::CONTENT_LENGTH, metadata.len())
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(body)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to build response: {}", e)))?;

    Ok(response)
}

// Weather Device API endpoints

use crate::config::models::{WeatherDeviceConfig, WeatherDataSource, WeatherSafetyThresholds};
use crate::weather::weather_monitor::WeatherMonitorStatus;

#[derive(Serialize)]
pub struct WeatherDeviceResponse {
    pub device_number: u32,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub auto_connect: bool,
    pub connected: bool,
    pub source_type: String,
    pub has_safety_thresholds: bool,
    pub safety_enabled: bool,
}

#[derive(Serialize)]
pub struct WeatherStatusResponse {
    pub device_number: u32,
    pub name: String,
    pub is_safe: bool,
    pub enabled: bool,
    pub last_update: Option<String>,
    pub error: Option<String>,
    pub measurements: HashMap<String, MeasurementInfo>,
}

#[derive(Serialize)]
pub struct MeasurementInfo {
    pub name: String,
    pub value: f64,
    pub threshold: f64,
    pub is_safe: bool,
}

// GET /api/weather/devices - List all weather devices
pub async fn get_weather_devices(
    State(state): State<SharedWebState>,
) -> Json<Vec<WeatherDeviceResponse>> {
    let config = state.config.read().await;
    let oc_devices = state.oc_devices.read().await;

    let mut devices = Vec::new();
    for (device_number, device_config) in &config.weather_devices {
        let connected = oc_devices.get(device_number)
            .map(|d| d.is_connected())
            .unwrap_or(false);

        let source_type = match &device_config.source {
            WeatherDataSource::Tempest { .. } => "tempest".to_string(),
            WeatherDataSource::WeatherUnderground { .. } => "weatherunderground".to_string(),
        };

        devices.push(WeatherDeviceResponse {
            device_number: *device_number,
            name: device_config.name.clone(),
            description: device_config.description.clone(),
            enabled: device_config.enabled,
            auto_connect: device_config.auto_connect,
            connected,
            source_type,
            has_safety_thresholds: device_config.safety_thresholds.is_some(),
            safety_enabled: device_config.safety_thresholds.as_ref()
                .map(|t| t.enabled)
                .unwrap_or(false),
        });
    }

    devices.sort_by_key(|d| d.device_number);
    Json(devices)
}

// GET /api/weather/devices/:id - Get specific weather device
pub async fn get_weather_device(
    Path(device_number): Path<u32>,
    State(state): State<SharedWebState>,
) -> Result<Json<WeatherDeviceConfig>, (StatusCode, String)> {
    let config = state.config.read().await;

    match config.weather_devices.get(&device_number) {
        Some(device) => Ok(Json(device.clone())),
        None => Err((StatusCode::NOT_FOUND, format!("Weather device {} not found", device_number))),
    }
}

// POST /api/weather/devices - Create new weather device
pub async fn create_weather_device(
    State(state): State<SharedWebState>,
    Json(device_config): Json<WeatherDeviceConfig>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut config = state.config.write().await;

    // Check if device already exists
    if config.weather_devices.contains_key(&device_config.device_number) {
        return Err((StatusCode::CONFLICT, format!("Weather device {} already exists", device_config.device_number)));
    }

    // Add device to config
    config.weather_devices.insert(device_config.device_number, device_config.clone());

    // Save configuration
    if let Err(e) = save_config(&config).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }

    // TODO: Reload weather devices without restarting service

    Ok(StatusCode::CREATED)
}

// PUT /api/weather/devices/:id - Update weather device
pub async fn update_weather_device(
    Path(device_number): Path<u32>,
    State(state): State<SharedWebState>,
    Json(device_config): Json<WeatherDeviceConfig>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut config = state.config.write().await;

    // Check if device exists
    if !config.weather_devices.contains_key(&device_number) {
        return Err((StatusCode::NOT_FOUND, format!("Weather device {} not found", device_number)));
    }

    // Ensure device number matches
    if device_config.device_number != device_number {
        return Err((StatusCode::BAD_REQUEST, "Device number mismatch".to_string()));
    }

    // Update device in config
    config.weather_devices.insert(device_number, device_config.clone());

    // Save configuration
    if let Err(e) = save_config(&config).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }

    // Update OC device if it exists
    let mut oc_devices = state.oc_devices.write().await;
    if let Some(device) = oc_devices.get(&device_number) {
        device.update_config(device_config);
    }

    // TODO: Reload weather monitors without restarting service

    Ok(StatusCode::OK)
}

// DELETE /api/weather/devices/:id - Delete weather device
pub async fn delete_weather_device(
    Path(device_number): Path<u32>,
    State(state): State<SharedWebState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut config = state.config.write().await;

    // Check if device exists
    if !config.weather_devices.contains_key(&device_number) {
        return Err((StatusCode::NOT_FOUND, format!("Weather device {} not found", device_number)));
    }

    // Remove device from config
    config.weather_devices.remove(&device_number);

    // Save configuration
    if let Err(e) = save_config(&config).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }

    // TODO: Remove OC device and weather monitor

    Ok(StatusCode::OK)
}

// POST /api/weather/devices/:id/toggle - Toggle device enabled state
pub async fn toggle_weather_device(
    Path(device_number): Path<u32>,
    State(state): State<SharedWebState>,
    Json(req): Json<ToggleEnabledRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut config = state.config.write().await;

    // Check if device exists
    let device_config = config.weather_devices.get_mut(&device_number)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Weather device {} not found", device_number)))?;

    device_config.enabled = req.enabled;

    // Save configuration
    if let Err(e) = save_config(&config).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }

    // TODO: Enable/disable device without restarting service

    Ok(StatusCode::OK)
}

// GET /api/weather/status - Get weather monitor safety statuses
pub async fn get_weather_status(
    State(state): State<SharedWebState>,
) -> Json<Vec<WeatherStatusResponse>> {
    let weather_monitors = state.weather_monitors.read().await;
    let statuses = weather_monitors.get_all_statuses().await;

    let mut response = Vec::new();
    for (device_number, status) in statuses {
        let measurements = status.measurements.iter().map(|(k, v)| {
            (k.clone(), MeasurementInfo {
                name: v.name.clone(),
                value: v.value,
                threshold: v.threshold,
                is_safe: v.is_safe,
            })
        }).collect();

        response.push(WeatherStatusResponse {
            device_number,
            name: status.name.clone(),
            is_safe: status.is_safe,
            enabled: status.enabled,
            last_update: status.last_update.map(|dt| dt.to_rfc3339()),
            error: status.error.clone(),
            measurements,
        });
    }

    response.sort_by_key(|s| s.device_number);
    Json(response)
}

// POST /api/weather/devices/:id/connect - Connect/disconnect device
pub async fn set_weather_device_connection(
    Path(device_number): Path<u32>,
    State(state): State<SharedWebState>,
    Json(req): Json<serde_json::Value>,
) -> Result<StatusCode, (StatusCode, String)> {
    let connected = req.get("connected")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing 'connected' field".to_string()))?;

    let oc_devices = state.oc_devices.read().await;
    let device = oc_devices.get(&device_number)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Weather device {} not found", device_number)))?;

    device.set_connected(connected);

    Ok(StatusCode::OK)
}
