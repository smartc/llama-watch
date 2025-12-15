use axum::{response::Json, extract::{State, Query}};
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use super::safety_monitor::{SafetyMonitor, SharedMonitorState};
use crate::weather::ObservingConditionsDevice;
use super::models::{AlpacaResponse, QueryParams};

/// Unified management state for all device types
pub struct ManagementState {
    pub safety_monitor: Arc<SafetyMonitor>,
    pub monitor_state: SharedMonitorState,
    pub oc_devices: Arc<RwLock<HashMap<u32, Arc<ObservingConditionsDevice>>>>,
}

pub type SharedManagementState = Arc<ManagementState>;

/// Generate a unique device ID based on hardware
fn generate_unique_id() -> String {
    // Try to get MAC address for stable unique ID
    match mac_address::get_mac_address() {
        Ok(Some(mac)) => {
            // Hash the MAC address to create a stable unique ID
            let mut hasher = Sha256::new();
            hasher.update(mac.bytes());
            let result = hasher.finalize();
            let hash_hex = hex::encode(&result[..8]); // Use first 8 bytes (16 hex chars)
            format!("llama-watch-{}", hash_hex)
        }
        _ => {
            // Fallback to a default ID if MAC address is not available
            "llama-watch-00000000-0000-0000-0000-000000000000".to_string()
        }
    }
}

/// Server description for Alpaca discovery
#[derive(Debug, Default, Serialize, Deserialize)]
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

/// GET /management/apiversions
pub async fn get_api_versions(
    Query(params): Query<QueryParams>,
) -> Json<AlpacaResponse<Vec<u32>>> {
    Json(AlpacaResponse::success(vec![1], params.client_transaction_i_d, 0))
}

/// GET /management/v1/description
pub async fn get_description(
    State(state): State<SharedManagementState>,
    Query(params): Query<QueryParams>,
) -> Json<AlpacaResponse<ServerDescription>> {
    Json(AlpacaResponse::success(
        ServerDescription {
            server_name: state.safety_monitor.device_name.clone(),
            manufacturer: "Corey Smart".to_string(),
            manufacturer_version: env!("CARGO_PKG_VERSION").to_string(),
            location: state.safety_monitor.location.clone(),
        },
        params.client_transaction_i_d,
        0,
    ))
}

/// GET /management/v1/configureddevices
pub async fn get_configured_devices(
    State(state): State<SharedManagementState>,
    Query(params): Query<QueryParams>,
) -> Json<AlpacaResponse<Vec<DeviceDescription>>> {
    let mut devices = vec![];

    // Add SafetyMonitor device
    devices.push(DeviceDescription {
        device_name: state.safety_monitor.device_name.clone(),
        device_type: "SafetyMonitor".to_string(),
        device_number: 0,
        unique_i_d: format!("{}-safetymonitor-0", generate_unique_id()),
    });

    // Add ObservingConditions devices
    let oc_devices = state.oc_devices.read().await;
    for (device_number, device) in oc_devices.iter() {
        devices.push(DeviceDescription {
            device_name: device.name.clone(),
            device_type: "ObservingConditions".to_string(),
            device_number: *device_number,
            unique_i_d: format!("{}-observingconditions-{}", generate_unique_id(), device_number),
        });
    }

    Json(AlpacaResponse::success(
        devices,
        params.client_transaction_i_d,
        0,
    ))
}
