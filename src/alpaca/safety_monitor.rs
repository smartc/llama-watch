use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    Form,
};
use parking_lot::RwLock;
use std::sync::{atomic::{AtomicU32, Ordering}, Arc};

use super::models::{AlpacaResponse, ConnectedRequest, QueryParams};
use crate::monitors::MonitorState;

pub struct SafetyMonitor {
    pub connected: RwLock<bool>,
    pub server_transaction_id: AtomicU32,
    pub device_name: String,
    pub description: String,
    pub driver_info: String,
    pub driver_version: String,
}

impl SafetyMonitor {
    pub fn new(device_name: String) -> Self {
        Self {
            connected: RwLock::new(false),
            server_transaction_id: AtomicU32::new(0),
            device_name,
            description: "LLAMA Safety Monitor - Monitors MQTT and ASCOM Alpaca endpoints".to_string(),
            driver_info: "LLAMA Safety Monitor Driver v0.1.0".to_string(),
            driver_version: "0.1.0".to_string(),
        }
    }

    fn next_transaction_id(&self) -> u32 {
        self.server_transaction_id.fetch_add(1, Ordering::SeqCst)
    }
}

pub type SafetyMonitorState = Arc<SafetyMonitor>;
pub type SharedMonitorState = Arc<RwLock<MonitorState>>;

// Handler state
pub struct AppState {
    pub safety_monitor: SafetyMonitorState,
    pub monitor_state: SharedMonitorState,
}

pub type SharedAppState = Arc<AppState>;

// GET /api/v1/safetymonitor/{device}/issafe
pub async fn is_safe(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedAppState>,
) -> Json<AlpacaResponse<bool>> {
    let server_id = state.safety_monitor.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ));
    }

    if !*state.safety_monitor.connected.read() {
        return Json(AlpacaResponse::error(
            params.client_transaction_i_d,
            server_id,
            0x407,
            "Device not connected".to_string(),
        ));
    }

    let is_safe = state.monitor_state.read().is_safe();

    Json(AlpacaResponse::success(
        is_safe,
        params.client_transaction_i_d,
        server_id,
    ))
}

// GET /api/v1/safetymonitor/{device}/connected
pub async fn get_connected(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedAppState>,
) -> Json<AlpacaResponse<bool>> {
    let server_id = state.safety_monitor.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ));
    }

    let connected = *state.safety_monitor.connected.read();

    Json(AlpacaResponse::success(
        connected,
        params.client_transaction_i_d,
        server_id,
    ))
}

// PUT /api/v1/safetymonitor/{device}/connected
pub async fn put_connected(
    Path(device): Path<u32>,
    State(state): State<SharedAppState>,
    Form(request): Form<ConnectedRequest>,
) -> Json<AlpacaResponse<()>> {
    let server_id = state.safety_monitor.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::error(
            request.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ));
    }

    *state.safety_monitor.connected.write() = request.connected;

    Json(AlpacaResponse::success(
        (),
        request.client_transaction_i_d,
        server_id,
    ))
}

// GET /api/v1/safetymonitor/{device}/name
pub async fn get_name(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedAppState>,
) -> Json<AlpacaResponse<String>> {
    let server_id = state.safety_monitor.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ));
    }

    Json(AlpacaResponse::success(
        state.safety_monitor.device_name.clone(),
        params.client_transaction_i_d,
        server_id,
    ))
}

// GET /api/v1/safetymonitor/{device}/description
pub async fn get_description(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedAppState>,
) -> Json<AlpacaResponse<String>> {
    let server_id = state.safety_monitor.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ));
    }

    Json(AlpacaResponse::success(
        state.safety_monitor.description.clone(),
        params.client_transaction_i_d,
        server_id,
    ))
}

// GET /api/v1/safetymonitor/{device}/driverinfo
pub async fn get_driver_info(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedAppState>,
) -> Json<AlpacaResponse<String>> {
    let server_id = state.safety_monitor.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ));
    }

    Json(AlpacaResponse::success(
        state.safety_monitor.driver_info.clone(),
        params.client_transaction_i_d,
        server_id,
    ))
}

// GET /api/v1/safetymonitor/{device}/driverversion
pub async fn get_driver_version(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedAppState>,
) -> Json<AlpacaResponse<String>> {
    let server_id = state.safety_monitor.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ));
    }

    Json(AlpacaResponse::success(
        state.safety_monitor.driver_version.clone(),
        params.client_transaction_i_d,
        server_id,
    ))
}

// GET /api/v1/safetymonitor/{device}/interfaceversion
pub async fn get_interface_version(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedAppState>,
) -> Json<AlpacaResponse<u32>> {
    let server_id = state.safety_monitor.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ));
    }

    Json(AlpacaResponse::success(
        1, // SafetyMonitor interface version 1
        params.client_transaction_i_d,
        server_id,
    ))
}

// GET /api/v1/safetymonitor/{device}/supportedactions
pub async fn get_supported_actions(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedAppState>,
) -> Json<AlpacaResponse<Vec<String>>> {
    let server_id = state.safety_monitor.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ));
    }

    Json(AlpacaResponse::success(
        vec![], // No custom actions supported
        params.client_transaction_i_d,
        server_id,
    ))
}
