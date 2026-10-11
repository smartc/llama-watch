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
    switches::SharedSwitchManager,
    weather::WeatherService,
};
use std::collections::HashMap;

pub type SharedConfig = Arc<RwLock<AppConfig>>;

pub struct WebState {
    pub config: SharedConfig,
    pub monitor_state: Arc<RwLock<MonitorState>>,
    pub switch_manager: Option<SharedSwitchManager>,
    pub debug_logger: Arc<DebugLogger>,
    pub safety_monitor: Arc<SafetyMonitor>,
    pub weather: Arc<WeatherService>,
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
    Json(mut new_config): Json<AppConfig>,
) -> Result<StatusCode, (StatusCode, String)> {
    // Accept configs in the pre-aggregate weather layout
    new_config.migrate();

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

    // Reload switches with new configuration
    if let Some(ref switch_manager) = state.switch_manager {
        let mut manager = switch_manager.write().await;
        manager.reload(&new_config);
    }

    // Update in-memory configuration
    *state.config.write().await = new_config;

    // Rebuild weather devices, monitors and polled sources
    state.weather.apply_config().await;

    Ok(StatusCode::OK)
}

// GET /api/status - Get all monitor statuses
pub async fn get_status(
    State(state): State<SharedWebState>,
) -> Json<serde_json::Value> {
    let monitor_state = state.monitor_state.read().await;
    let config = state.config.read().await;

    let mut statuses = monitor_state.get_all_statuses();
    let weather_safe = state.weather.monitors.read().await.are_all_safe().await;
    let is_safe = monitor_state.is_safe() && weather_safe;

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
                include_in_safety: mqtt_config.include_in_safety,
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
                include_in_safety: alpaca_config.include_in_safety,
            });
        }
    }

    // Sort monitors alphabetically by name for consistent display order
    statuses.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    // Get switch statuses if switch manager is available
    let switches = if let Some(ref switch_manager) = state.switch_manager {
        let mut manager = switch_manager.write().await;
        manager.get_all_statuses().await
    } else {
        Vec::new()
    };

    Json(serde_json::json!({
        "is_safe": is_safe,
        "is_connected": is_connected,
        "monitors": statuses,
        "switches": switches,
    }))
}

#[derive(Deserialize)]
pub struct ToggleEnabledRequest {
    enabled: bool,
}

#[derive(Deserialize)]
pub struct ToggleSafetyRequest {
    include_in_safety: bool,
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
    drop(monitor_state);

    // Reload switches
    if let Some(ref switch_manager) = state.switch_manager {
        let mut manager = switch_manager.write().await;
        manager.reload(&config);
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
    drop(monitor_state);

    // Reload switches
    if let Some(ref switch_manager) = state.switch_manager {
        let mut manager = switch_manager.write().await;
        manager.reload(&config);
    }

    Ok(StatusCode::OK)
}

// POST /api/monitors/mqtt/:id/toggle-safety - Toggle MQTT monitor include_in_safety state
pub async fn toggle_mqtt_monitor_safety(
    Path(id): Path<String>,
    State(state): State<SharedWebState>,
    Json(req): Json<ToggleSafetyRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut config = state.config.write().await;

    // Find and update the monitor
    if let Some(monitor) = config.mqtt_monitors.get_mut(&id) {
        monitor.include_in_safety = req.include_in_safety;
    } else {
        return Err((StatusCode::NOT_FOUND, format!("Monitor '{}' not found", id)));
    }

    // Save configuration
    if let Err(e) = save_config(&config).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }

    // Reload monitors to pick up changed include_in_safety
    let mut monitor_state = state.monitor_state.write().await;
    if let Err(e) = monitor_state.reload(&config).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to reload monitors: {}", e)));
    }

    Ok(StatusCode::OK)
}

// POST /api/monitors/alpaca/:id/toggle-safety - Toggle Alpaca monitor include_in_safety state
pub async fn toggle_alpaca_monitor_safety(
    Path(id): Path<String>,
    State(state): State<SharedWebState>,
    Json(req): Json<ToggleSafetyRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut config = state.config.write().await;

    // Find and update the monitor
    if let Some(monitor) = config.alpaca_monitors.get_mut(&id) {
        monitor.include_in_safety = req.include_in_safety;
    } else {
        return Err((StatusCode::NOT_FOUND, format!("Monitor '{}' not found", id)));
    }

    // Save configuration
    if let Err(e) = save_config(&config).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }

    // Reload monitors to pick up changed include_in_safety
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

use crate::config::models::{WeatherAggregateConfig, WeatherDataSource, WeatherDeviceConfig};
use crate::weather::observing_conditions::WeatherDeviceKind;
use crate::weather::service::exposed_devices;

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
    pub expose_device: bool,
    pub exposed: bool, // Currently served as its own ObservingConditions device
}

#[derive(Serialize)]
pub struct WeatherAggregateResponse {
    #[serde(flatten)]
    pub config: WeatherAggregateConfig,
    pub exposed: bool,
    pub connected: bool,
}

#[derive(Serialize)]
pub struct WeatherStatusResponse {
    pub device_number: u32,
    pub name: String,
    pub kind: &'static str, // "aggregate" or "source"
    pub is_safe: bool,
    pub enabled: bool,
    pub exposed: bool,
    pub safety_monitored: bool,
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
    pub is_monitored: bool, // Whether this measurement is included in safety determination
    pub timeout_seconds: Option<i64>, // Negative = ignore timeout
    pub last_update: Option<String>,
    pub source_device: u32,
    pub source_name: String,
}

/// Apply a change to the weather configuration: validate, save, then rebuild
/// weather devices, monitors and polled sources. Nothing changes if any step fails.
async fn update_weather_config<F>(state: &SharedWebState, change: F) -> Result<(), (StatusCode, String)>
where
    F: FnOnce(&mut AppConfig) -> Result<(), (StatusCode, String)>,
{
    let mut config = state.config.write().await;
    let mut updated = config.clone();
    change(&mut updated)?;
    updated.validate().map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    save_config(&updated).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    *config = updated;
    drop(config);

    state.weather.apply_config().await;
    Ok(())
}

fn source_type(source: &WeatherDataSource) -> String {
    match source {
        WeatherDataSource::Tempest { .. } => "tempest",
        WeatherDataSource::Ecowitt { .. } => "ecowitt",
        WeatherDataSource::Alpaca { .. } => "alpaca",
        WeatherDataSource::WeatherUnderground { .. } => "weatherunderground",
    }.to_string()
}

// GET /api/weather/devices - List all weather sources
pub async fn get_weather_devices(
    State(state): State<SharedWebState>,
) -> Json<Vec<WeatherDeviceResponse>> {
    let config = state.config.read().await;
    let oc_devices = state.weather.devices.read().await;
    let exposed: Vec<u32> = exposed_devices(&config).iter().map(|d| d.kind.device_number()).collect();

    let mut devices = Vec::new();
    for (device_number, device_config) in &config.weather_devices {
        let connected = oc_devices.get(device_number)
            .map(|d| d.is_connected())
            .unwrap_or(false);

        devices.push(WeatherDeviceResponse {
            device_number: *device_number,
            name: device_config.name.clone(),
            description: device_config.description.clone(),
            enabled: device_config.enabled,
            auto_connect: device_config.auto_connect,
            connected,
            source_type: source_type(&device_config.source),
            has_safety_thresholds: device_config.safety_thresholds.is_some(),
            safety_enabled: device_config.safety_thresholds.as_ref()
                .map(|t| t.enabled)
                .unwrap_or(false),
            expose_device: device_config.expose_device,
            exposed: exposed.contains(device_number),
        });
    }

    devices.sort_by_key(|d| d.device_number);
    Json(devices)
}

// GET /api/weather/devices/:id - Get specific weather source
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

// POST /api/weather/devices - Create new weather source (device_number 0 = next free number)
pub async fn create_weather_device(
    State(state): State<SharedWebState>,
    Json(mut device_config): Json<WeatherDeviceConfig>,
) -> Result<(StatusCode, Json<WeatherDeviceConfig>), (StatusCode, String)> {
    let mut created = None;
    update_weather_config(&state, |config| {
        if device_config.device_number == 0 {
            device_config.device_number = config.next_weather_device_number();
        }
        if config.weather_devices.contains_key(&device_config.device_number) {
            return Err((StatusCode::CONFLICT, format!("Weather device {} already exists", device_config.device_number)));
        }
        config.weather_devices.insert(device_config.device_number, device_config.clone());
        // An explicit priority list only lists the sources that existed when it was
        // saved; add the new source at the end so the aggregate uses it
        if let Some(aggregate) = config.weather_aggregate.as_mut() {
            if !aggregate.source_priority.is_empty() {
                aggregate.source_priority.push(device_config.device_number);
            }
        }
        created = Some(device_config);
        Ok(())
    }).await?;

    Ok((StatusCode::CREATED, Json(created.expect("set when the update succeeds"))))
}

// PUT /api/weather/devices/:id - Update weather source
pub async fn update_weather_device(
    Path(device_number): Path<u32>,
    State(state): State<SharedWebState>,
    Json(device_config): Json<WeatherDeviceConfig>,
) -> Result<StatusCode, (StatusCode, String)> {
    update_weather_config(&state, |config| {
        if !config.weather_devices.contains_key(&device_number) {
            return Err((StatusCode::NOT_FOUND, format!("Weather device {} not found", device_number)));
        }
        if device_config.device_number != device_number {
            return Err((StatusCode::BAD_REQUEST, "Device number mismatch".to_string()));
        }
        config.weather_devices.insert(device_number, device_config);
        Ok(())
    }).await?;

    Ok(StatusCode::OK)
}

// DELETE /api/weather/devices/:id - Delete weather source
pub async fn delete_weather_device(
    Path(device_number): Path<u32>,
    State(state): State<SharedWebState>,
) -> Result<StatusCode, (StatusCode, String)> {
    update_weather_config(&state, |config| {
        if config.weather_devices.remove(&device_number).is_none() {
            return Err((StatusCode::NOT_FOUND, format!("Weather device {} not found", device_number)));
        }
        if let Some(aggregate) = config.weather_aggregate.as_mut() {
            aggregate.remove_source(device_number);
        }
        Ok(())
    }).await?;

    Ok(StatusCode::OK)
}

// POST /api/weather/devices/:id/toggle - Toggle source enabled state
pub async fn toggle_weather_device(
    Path(device_number): Path<u32>,
    State(state): State<SharedWebState>,
    Json(req): Json<ToggleEnabledRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    update_weather_config(&state, |config| {
        let device_config = config.weather_devices.get_mut(&device_number)
            .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Weather device {} not found", device_number)))?;
        device_config.enabled = req.enabled;
        Ok(())
    }).await?;

    Ok(StatusCode::OK)
}

// GET /api/weather/aggregate - Get aggregate device (device 0) configuration
pub async fn get_weather_aggregate(
    State(state): State<SharedWebState>,
) -> Json<WeatherAggregateResponse> {
    let config = state.config.read().await;
    let aggregate = state.weather.devices.read().await.get(&0).cloned();

    Json(WeatherAggregateResponse {
        config: config.weather_aggregate.clone().unwrap_or_default(),
        exposed: aggregate.is_some(),
        connected: aggregate.map(|d| d.is_connected()).unwrap_or(false),
    })
}

// PUT /api/weather/aggregate - Update aggregate device (device 0) configuration
pub async fn update_weather_aggregate(
    State(state): State<SharedWebState>,
    Json(aggregate): Json<WeatherAggregateConfig>,
) -> Result<StatusCode, (StatusCode, String)> {
    update_weather_config(&state, |config| {
        config.weather_aggregate = Some(aggregate);
        Ok(())
    }).await?;

    Ok(StatusCode::OK)
}

// GET /api/weather/status - Readings and safety status for the aggregate and each enabled source
pub async fn get_weather_status(
    State(state): State<SharedWebState>,
) -> Json<Vec<WeatherStatusResponse>> {
    let config = state.config.read().await;
    let statuses = state.weather.monitors.read().await.get_all_statuses().await;
    let exposed: Vec<u32> = exposed_devices(&config).iter().map(|d| d.kind.device_number()).collect();

    let mut kinds = Vec::new();
    if config.weather_devices.values().any(|d| d.enabled) {
        if let Some(aggregate) = &config.weather_aggregate {
            kinds.push((WeatherDeviceKind::Aggregate, aggregate.name.clone()));
        }
    }
    let mut sources: Vec<_> = config.weather_devices.values().filter(|d| d.enabled).collect();
    sources.sort_by_key(|d| d.device_number);
    kinds.extend(sources.into_iter().map(|d| (WeatherDeviceKind::Source(d.device_number), d.name.clone())));

    let mut response = Vec::new();
    for (kind, name) in kinds {
        let device_number = kind.device_number();
        let readings = state.weather.data.readings_with(&config, kind).await;

        // Get safety monitoring status for this device (if exists)
        let safety_status = statuses.get(&device_number);

        let mut measurements = HashMap::new();
        for (property, reading) in &readings {
            // Keys must match those used in weather_monitor.rs for safety threshold lookups
            let key = property.label();
            let monitored = safety_status.and_then(|status| status.measurements.get(key));
            measurements.insert(key.to_string(), MeasurementInfo {
                name: property.display_name(),
                value: reading.value,
                threshold: monitored.map(|m| m.threshold).unwrap_or(0.0),
                is_safe: monitored.map(|m| m.is_safe).unwrap_or(true),
                is_monitored: monitored.is_some(),
                timeout_seconds: monitored.and_then(|m| m.timeout_seconds),
                last_update: Some(reading.timestamp.to_rfc3339()),
                source_device: reading.source_device,
                source_name: config.weather_devices.get(&reading.source_device)
                    .map(|d| d.name.clone())
                    .unwrap_or_default(),
            });
        }

        // Determine overall safety status
        let (is_safe, error) = if let Some(status) = safety_status {
            (status.is_safe, status.error.clone())
        } else {
            (true, None) // No safety monitoring = always safe
        };

        let last_update = readings.values()
            .map(|r| r.timestamp)
            .max()
            .map(|ts| ts.to_rfc3339());

        response.push(WeatherStatusResponse {
            device_number,
            name,
            kind: match kind {
                WeatherDeviceKind::Aggregate => "aggregate",
                WeatherDeviceKind::Source(_) => "source",
            },
            is_safe,
            enabled: true,
            exposed: exposed.contains(&device_number),
            safety_monitored: safety_status.is_some(),
            last_update,
            error,
            measurements,
        });
    }

    Json(response)
}

#[derive(Deserialize)]
pub struct AlpacaDetectRequest {
    pub host: String,
    #[serde(default = "default_alpaca_detect_port")]
    pub port: u16,
    #[serde(default)]
    pub device_number: u32,
}

fn default_alpaca_detect_port() -> u16 {
    11111
}

// POST /api/weather/alpaca/detect - List the properties an Alpaca ObservingConditions device implements
pub async fn detect_alpaca_weather_device(
    Json(req): Json<AlpacaDetectRequest>,
) -> Result<Json<crate::weather::alpaca::DetectedDevice>, (StatusCode, String)> {
    crate::weather::alpaca::detect(&req.host, req.port, req.device_number).await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("{:#}", e)))
}

// POST /api/weather/devices/:id/connect - Connect/disconnect an ObservingConditions device
pub async fn set_weather_device_connection(
    Path(device_number): Path<u32>,
    State(state): State<SharedWebState>,
    Json(req): Json<serde_json::Value>,
) -> Result<StatusCode, (StatusCode, String)> {
    let connected = req.get("connected")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing 'connected' field".to_string()))?;

    let oc_devices = state.weather.devices.read().await;
    let device = oc_devices.get(&device_number)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Weather device {} not found", device_number)))?;

    device.set_connected(connected);

    Ok(StatusCode::OK)
}

// ============================================================================
// Alpaca Discovery API endpoints
// ============================================================================

use crate::alpaca::discovery_client::{DiscoveryClient, DiscoveredDevice};

#[derive(Deserialize)]
pub struct DiscoverRequest {
    #[serde(default = "default_discovery_timeout")]
    pub timeout_seconds: u64,
}

fn default_discovery_timeout() -> u64 {
    5
}

// GET /api/discovery/scan - Discover Alpaca devices on the network
pub async fn discover_devices(
) -> Json<Vec<DiscoveredDevice>> {
    let client = DiscoveryClient::new(5);
    let devices = client.discover().await;
    Json(devices)
}

// POST /api/discovery/scan - Discover Alpaca devices with custom timeout
pub async fn discover_devices_with_options(
    Json(req): Json<DiscoverRequest>,
) -> Json<Vec<DiscoveredDevice>> {
    let client = DiscoveryClient::new(req.timeout_seconds);
    let devices = client.discover().await;
    Json(devices)
}

#[derive(Deserialize)]
pub struct ProbeRequest {
    pub host: String,
    pub port: u16,
}

// POST /api/discovery/probe - Probe a specific host for Alpaca devices
pub async fn probe_device(
    Json(req): Json<ProbeRequest>,
) -> Result<Json<DiscoveredDevice>, (StatusCode, String)> {
    let client = DiscoveryClient::new(5);
    match client.probe_host(&req.host, req.port).await {
        Some(device) => Ok(Json(device)),
        None => Err((StatusCode::NOT_FOUND, "No Alpaca server found at specified address".to_string())),
    }
}

// ============================================================================
// Monitor Groups API endpoints
// ============================================================================

use crate::config::models::MonitorGroupConfig;
use crate::monitors::MonitorGroupStatus;

// GET /api/groups - Get all monitor groups
pub async fn get_monitor_groups(
    State(state): State<SharedWebState>,
) -> Json<HashMap<String, MonitorGroupConfig>> {
    let config = state.config.read().await;
    Json(config.monitor_groups.clone())
}

// GET /api/groups/status - Get status of all monitor groups
pub async fn get_monitor_group_statuses(
    State(state): State<SharedWebState>,
) -> Json<Vec<MonitorGroupStatus>> {
    let monitor_state = state.monitor_state.read().await;
    Json(monitor_state.get_group_statuses())
}

// GET /api/groups/:id - Get a specific monitor group
pub async fn get_monitor_group(
    Path(id): Path<String>,
    State(state): State<SharedWebState>,
) -> Result<Json<MonitorGroupConfig>, (StatusCode, String)> {
    let config = state.config.read().await;
    match config.monitor_groups.get(&id) {
        Some(group) => Ok(Json(group.clone())),
        None => Err((StatusCode::NOT_FOUND, format!("Monitor group '{}' not found", id))),
    }
}

// POST /api/groups - Create a new monitor group
pub async fn create_monitor_group(
    State(state): State<SharedWebState>,
    Json(group): Json<MonitorGroupConfig>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut config = state.config.write().await;

    // Check if group already exists
    if config.monitor_groups.contains_key(&group.id) {
        return Err((StatusCode::CONFLICT, format!("Monitor group '{}' already exists", group.id)));
    }

    // Add group to config
    let group_id = group.id.clone();
    config.monitor_groups.insert(group_id.clone(), group);

    // Validate configuration
    if let Err(e) = config.validate() {
        // Rollback
        config.monitor_groups.remove(&group_id);
        return Err((StatusCode::BAD_REQUEST, e.to_string()));
    }

    // Save configuration
    if let Err(e) = save_config(&config).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }

    // Release config lock before acquiring monitor_state lock
    let config_clone = config.clone();
    drop(config);

    // Reload monitors to pick up new groups
    let mut monitor_state = state.monitor_state.write().await;
    if let Err(e) = monitor_state.reload(&config_clone).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to reload monitors: {}", e)));
    }

    Ok(StatusCode::CREATED)
}

// PUT /api/groups/:id - Update a monitor group
pub async fn update_monitor_group(
    Path(id): Path<String>,
    State(state): State<SharedWebState>,
    Json(group): Json<MonitorGroupConfig>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut config = state.config.write().await;

    // Check if group exists
    if !config.monitor_groups.contains_key(&id) {
        return Err((StatusCode::NOT_FOUND, format!("Monitor group '{}' not found", id)));
    }

    // Ensure ID matches
    if group.id != id {
        return Err((StatusCode::BAD_REQUEST, "Group ID mismatch".to_string()));
    }

    // Update group
    config.monitor_groups.insert(id.clone(), group);

    // Validate configuration
    if let Err(e) = config.validate() {
        return Err((StatusCode::BAD_REQUEST, e.to_string()));
    }

    // Save configuration
    if let Err(e) = save_config(&config).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }

    // Release config lock before acquiring monitor_state lock
    let config_clone = config.clone();
    drop(config);

    // Reload monitors to pick up updated groups
    let mut monitor_state = state.monitor_state.write().await;
    if let Err(e) = monitor_state.reload(&config_clone).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to reload monitors: {}", e)));
    }

    Ok(StatusCode::OK)
}

// DELETE /api/groups/:id - Delete a monitor group
pub async fn delete_monitor_group(
    Path(id): Path<String>,
    State(state): State<SharedWebState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut config = state.config.write().await;

    // Check if group exists
    if !config.monitor_groups.contains_key(&id) {
        return Err((StatusCode::NOT_FOUND, format!("Monitor group '{}' not found", id)));
    }

    // Remove group
    config.monitor_groups.remove(&id);

    // Save configuration
    if let Err(e) = save_config(&config).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }

    // Release config lock before acquiring monitor_state lock
    let config_clone = config.clone();
    drop(config);

    // Reload monitors
    let mut monitor_state = state.monitor_state.write().await;
    if let Err(e) = monitor_state.reload(&config_clone).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to reload monitors: {}", e)));
    }

    Ok(StatusCode::OK)
}
