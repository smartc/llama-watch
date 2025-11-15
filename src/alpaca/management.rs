use axum::{response::Json, extract::State};
use serde::{Deserialize, Serialize};
use super::safety_monitor::SharedAppState;

/// Server description for Alpaca discovery
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ServerDescription {
    pub server_name: String,
    pub manufacturer: String,
    pub manufacturer_version: String,
    pub location: String,
}

/// Device description for device list
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DeviceDescription {
    pub device_name: String,
    pub device_type: String,
    pub device_number: u32,
    pub unique_i_d: String,
}

/// Response for configured devices endpoint
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ConfiguredDevicesResponse {
    pub value: Vec<DeviceDescription>,
}

/// GET /management/apiversions
pub async fn get_api_versions() -> Json<Vec<u32>> {
    Json(vec![1])
}

/// GET /management/v1/description
pub async fn get_description(
    State(state): State<SharedAppState>,
) -> Json<ServerDescription> {
    Json(ServerDescription {
        server_name: state.safety_monitor.device_name.clone(),
        manufacturer: "LLAMA".to_string(),
        manufacturer_version: "0.1.0".to_string(),
        location: "".to_string(),
    })
}

/// GET /management/v1/configureddevices
pub async fn get_configured_devices(
    State(state): State<SharedAppState>,
) -> Json<ConfiguredDevicesResponse> {
    Json(ConfiguredDevicesResponse {
        value: vec![DeviceDescription {
            device_name: state.safety_monitor.device_name.clone(),
            device_type: "SafetyMonitor".to_string(),
            device_number: 0,
            unique_i_d: "llama-safety-monitor-00000000-0000-0000-0000-000000000000".to_string(),
        }],
    })
}
