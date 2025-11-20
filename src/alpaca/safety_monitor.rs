use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    Form,
};
use parking_lot::RwLock as SyncRwLock;
use std::sync::{atomic::{AtomicU32, Ordering}, Arc};
use tokio::sync::RwLock;

use super::models::{AlpacaResponse, ConnectedRequest, QueryParams, ActionRequest, CommandRequest};
use crate::monitors::MonitorState;

pub struct SafetyMonitor {
    pub connected: SyncRwLock<bool>,
    pub server_transaction_id: AtomicU32,
    pub device_name: String,
    pub description: String,
    pub driver_info: String,
    pub driver_version: String,
}

impl SafetyMonitor {
    pub fn new(device_name: String) -> Self {
        Self {
            connected: SyncRwLock::new(false), // Start disconnected per ASCOM standard
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

    /// Check if device is truly connected (user-enabled AND monitors ready)
    pub async fn is_connected(&self, monitor_state: &SharedMonitorState) -> bool {
        *self.connected.read() && monitor_state.read().await.is_ready()
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

    if !state.safety_monitor.is_connected(&state.monitor_state).await {
        return Json(AlpacaResponse::error(
            params.client_transaction_i_d,
            server_id,
            0x407,
            "Device not connected".to_string(),
        ));
    }

    let monitor_state = state.monitor_state.read().await;
    let is_safe = monitor_state.is_safe();

    // If unsafe, get the detailed comments and include them in the Comments field
    if !is_safe {
        if let Some(comments) = monitor_state.get_safety_comments() {
            return Json(AlpacaResponse::success_with_comments(
                is_safe,
                params.client_transaction_i_d,
                server_id,
                comments,
            ));
        }
    }

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

    // Connected = user-enabled AND monitors are ready (configured + have data)
    let connected = state.safety_monitor.is_connected(&state.monitor_state).await;

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

// PUT /api/v1/safetymonitor/{device}/action
pub async fn put_action(
    Path(device): Path<u32>,
    State(state): State<SharedAppState>,
    Form(request): Form<ActionRequest>,
) -> Json<AlpacaResponse<String>> {
    let server_id = state.safety_monitor.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::error(
            request.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ));
    }

    if !state.safety_monitor.is_connected(&state.monitor_state).await {
        return Json(AlpacaResponse::error(
            request.client_transaction_i_d,
            server_id,
            0x407,
            "Device not connected".to_string(),
        ));
    }

    // No custom actions are supported
    Json(AlpacaResponse::error(
        request.client_transaction_i_d,
        server_id,
        0x40C,
        format!("Action '{}' is not supported", request.action),
    ))
}

// PUT /api/v1/safetymonitor/{device}/commandblind
pub async fn put_command_blind(
    Path(device): Path<u32>,
    State(state): State<SharedAppState>,
    Form(request): Form<CommandRequest>,
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

    if !state.safety_monitor.is_connected(&state.monitor_state).await {
        return Json(AlpacaResponse::error(
            request.client_transaction_i_d,
            server_id,
            0x407,
            "Device not connected".to_string(),
        ));
    }

    // CommandBlind is not implemented
    Json(AlpacaResponse::error(
        request.client_transaction_i_d,
        server_id,
        0x40C,
        "CommandBlind is not implemented".to_string(),
    ))
}

// PUT /api/v1/safetymonitor/{device}/commandbool
pub async fn put_command_bool(
    Path(device): Path<u32>,
    State(state): State<SharedAppState>,
    Form(request): Form<CommandRequest>,
) -> Json<AlpacaResponse<bool>> {
    let server_id = state.safety_monitor.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::error(
            request.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ));
    }

    if !state.safety_monitor.is_connected(&state.monitor_state).await {
        return Json(AlpacaResponse::error(
            request.client_transaction_i_d,
            server_id,
            0x407,
            "Device not connected".to_string(),
        ));
    }

    // CommandBool is not implemented
    Json(AlpacaResponse::error(
        request.client_transaction_i_d,
        server_id,
        0x40C,
        "CommandBool is not implemented".to_string(),
    ))
}

// PUT /api/v1/safetymonitor/{device}/commandstring
pub async fn put_command_string(
    Path(device): Path<u32>,
    State(state): State<SharedAppState>,
    Form(request): Form<CommandRequest>,
) -> Json<AlpacaResponse<String>> {
    let server_id = state.safety_monitor.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::error(
            request.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ));
    }

    if !state.safety_monitor.is_connected(&state.monitor_state).await {
        return Json(AlpacaResponse::error(
            request.client_transaction_i_d,
            server_id,
            0x407,
            "Device not connected".to_string(),
        ));
    }

    // CommandString is not implemented
    Json(AlpacaResponse::error(
        request.client_transaction_i_d,
        server_id,
        0x40C,
        "CommandString is not implemented".to_string(),
    ))
}
