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
    weather::{WeatherMonitorManager, ObservingConditionsDevice, tempest::TempestData},
};
use std::collections::HashMap;

pub type SharedConfig = Arc<RwLock<AppConfig>>;

pub struct WebState {
    pub config: SharedConfig,
    pub monitor_state: Arc<RwLock<MonitorState>>,
    pub switch_manager: Option<SharedSwitchManager>,
    pub debug_logger: Arc<DebugLogger>,
    pub safety_monitor: Arc<SafetyMonitor>,
    pub oc_devices: Arc<RwLock<HashMap<u32, Arc<ObservingConditionsDevice>>>>,
    pub weather_monitors: Arc<RwLock<WeatherMonitorManager>>,
    pub tempest_data: Arc<RwLock<TempestData>>,
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

    // Reload switches with new configuration
    if let Some(ref switch_manager) = state.switch_manager {
        let mut manager = switch_manager.write().await;
        manager.reload(&new_config);
    }

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
    pub is_monitored: bool, // Whether this measurement is included in safety determination
    pub timeout_seconds: Option<i64>, // Negative = ignore timeout
    pub last_update: Option<String>,
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

    // Release config lock before acquiring other locks
    drop(config);

    // Create ObservingConditions device if enabled
    if device_config.enabled {
        let device = Arc::new(ObservingConditionsDevice::new(device_config.clone()));
        let mut oc_devices = state.oc_devices.write().await;
        oc_devices.insert(device_config.device_number, device);
    }

    // Reinitialize weather monitor for this device if safety thresholds are enabled
    if let Some(ref thresholds) = device_config.safety_thresholds {
        if thresholds.enabled {
            let mut weather_monitors = state.weather_monitors.write().await;
            let mut devices_map = HashMap::new();
            devices_map.insert(device_config.device_number, device_config.clone());
            weather_monitors.initialize(&devices_map, state.tempest_data.clone()).await;
        }
    }

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

    // Release config lock before acquiring other locks
    drop(config);

    // Update or create OC device
    let mut oc_devices = state.oc_devices.write().await;
    if let Some(device) = oc_devices.get(&device_number) {
        // Update existing device
        device.update_config(device_config.clone());
    } else if device_config.enabled {
        // Create new device if it doesn't exist and is enabled
        let device = Arc::new(ObservingConditionsDevice::new(device_config.clone()));
        oc_devices.insert(device_number, device);
    }
    drop(oc_devices);

    // Reinitialize weather monitor for this device if safety thresholds are enabled
    if let Some(ref thresholds) = device_config.safety_thresholds {
        if thresholds.enabled {
            let mut weather_monitors = state.weather_monitors.write().await;
            let mut devices_map = HashMap::new();
            devices_map.insert(device_number, device_config.clone());
            weather_monitors.initialize(&devices_map, state.tempest_data.clone()).await;
        }
    }

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

    // Release config lock before acquiring other locks
    drop(config);

    // Remove ObservingConditions device
    let mut oc_devices = state.oc_devices.write().await;
    oc_devices.remove(&device_number);
    drop(oc_devices);

    // Remove weather monitor for this device
    let mut weather_monitors = state.weather_monitors.write().await;
    weather_monitors.remove_monitor(device_number).await;

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
    use crate::config::models::WeatherDataSource;

    let config = state.config.read().await;
    let weather_monitors = state.weather_monitors.read().await;
    let tempest_data = state.tempest_data.read().await;
    let statuses = weather_monitors.get_all_statuses().await;

    let mut response = Vec::new();

    // Process each configured weather device
    for (device_number, device_config) in &config.weather_devices {
        if !device_config.enabled {
            continue; // Skip disabled devices
        }

        // Get actual weather observation data
        let observation = match &device_config.source {
            WeatherDataSource::Tempest { serial_number } => {
                if let Some(sn) = serial_number {
                    tempest_data.observations.get(sn).cloned()
                } else {
                    tempest_data.observations.values().next().cloned()
                }
            }
            WeatherDataSource::WeatherUnderground { .. } => None,
        };

        // Get safety monitoring status for this device (if exists)
        let safety_status = statuses.get(device_number);

        let mut measurements = HashMap::new();

        if let Some(ref obs) = observation {
            let last_update_str = obs.timestamp.to_rfc3339();

            // Helper function to add a measurement
            let mut add_measurement = |key: &str, name: &str, value: f64| {
                // Check if this measurement is being monitored for safety
                let (threshold, is_safe, is_monitored, timeout_seconds) = if let Some(status) = safety_status {
                    if let Some(m) = status.measurements.get(key) {
                        (m.threshold, m.is_safe, true, m.timeout_seconds)
                    } else {
                        (0.0, true, false, None) // Not monitored
                    }
                } else {
                    (0.0, true, false, None) // No safety monitoring configured
                };

                measurements.insert(key.to_string(), MeasurementInfo {
                    name: name.to_string(),
                    value,
                    threshold,
                    is_safe,
                    is_monitored,
                    timeout_seconds,
                    last_update: Some(last_update_str.clone()),
                });
            };

            // Add all available measurements
            // Keys must match those used in weather_monitor.rs for safety threshold lookups
            add_measurement("Temperature", "Temperature (°C)", obs.temperature);
            add_measurement("Humidity", "Humidity (%)", obs.humidity);
            add_measurement("Pressure", "Pressure (MB)", obs.pressure);
            add_measurement("Wind Speed", "Wind Speed (m/s)", obs.wind_avg);
            add_measurement("Wind Gust", "Wind Gust (m/s)", obs.wind_gust);
            add_measurement("Wind Direction", "Wind Direction (°)", obs.wind_direction);

            // Calculate rain rate (mm/hour)
            let rain_rate = if let Some(prev_obs) = tempest_data.previous_observations.get(&obs.serial_number) {
                obs.calculate_rain_rate(Some(prev_obs))
            } else {
                0.0
            };
            add_measurement("Rain Rate", "Rain Rate (mm/hr)", rain_rate);

            // Calculate dew point
            let dew_point = calculate_dew_point(obs.temperature, obs.humidity);
            add_measurement("Dew Point", "Dew Point (°C)", dew_point);

            // Convert illuminance to sky brightness (mag/arcsec²)
            let sky_brightness = lux_to_sky_brightness(obs.illuminance);
            add_measurement("Sky Brightness", "Sky Brightness (mag/arcsec²)", sky_brightness);
        }

        // Determine overall safety status
        let (is_safe, error) = if let Some(status) = safety_status {
            (status.is_safe, status.error.clone())
        } else {
            (true, None) // No safety monitoring = always safe
        };

        let last_update = observation.as_ref()
            .map(|obs| obs.timestamp.to_rfc3339());

        response.push(WeatherStatusResponse {
            device_number: *device_number,
            name: device_config.name.clone(),
            is_safe,
            enabled: device_config.enabled,
            last_update,
            error,
            measurements,
        });
    }

    response.sort_by_key(|s| s.device_number);
    Json(response)
}

// Helper function to calculate dew point from temperature and humidity
fn calculate_dew_point(temp_c: f64, humidity: f64) -> f64 {
    let a = 17.27;
    let b = 237.7;
    let alpha = ((a * temp_c) / (b + temp_c)) + (humidity / 100.0).ln();
    (b * alpha) / (a - alpha)
}

// Helper function to convert lux to sky brightness (mag/arcsec²)
fn lux_to_sky_brightness(lux: f64) -> f64 {
    if lux <= 0.0 {
        return 22.0; // Very dark sky
    }
    // Empirical conversion: mag/arcsec² = 22.0 - 2.5 * log10(lux)
    22.0 - 2.5 * lux.log10()
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
