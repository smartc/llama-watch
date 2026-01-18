use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    Json,
};
use parking_lot::RwLock as SyncRwLock;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use tokio::sync::RwLock;

use super::models::{AlpacaForm, AlpacaResponse, ConnectedRequest, QueryParams, ActionRequest, CommandRequest};
use crate::config::models::SwitchDeviceConfig;
use crate::switches::SharedSwitchManager;

/// ASCOM Alpaca Switch Device
/// Exposes aggregated switches with configurable logic (ANY, ALL, MIN_OF)
pub struct SwitchDevice {
    pub connected: SyncRwLock<bool>,
    pub server_transaction_id: AtomicU32,
    pub device_name: String,
    pub description: String,
    pub driver_info: String,
    pub driver_version: String,
}

impl SwitchDevice {
    pub fn new(config: &SwitchDeviceConfig) -> Self {
        let device_name = config
            .name
            .clone()
            .unwrap_or_else(|| "LLAMA Switch Device".to_string());
        let description = config.description.clone().unwrap_or_else(|| {
            "Aggregated switches with configurable logic for automation".to_string()
        });
        let auto_connect = config.auto_connect;

        Self {
            connected: SyncRwLock::new(auto_connect),
            server_transaction_id: AtomicU32::new(0),
            device_name,
            description,
            driver_info: format!("LLAMA Switch Driver v{}", env!("CARGO_PKG_VERSION")),
            driver_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    fn next_transaction_id(&self) -> u32 {
        self.server_transaction_id.fetch_add(1, Ordering::SeqCst)
    }

    pub fn is_connected(&self) -> bool {
        *self.connected.read()
    }
}

pub type SwitchDeviceState = Arc<SwitchDevice>;

/// Application state for Switch device handlers
pub struct SwitchAppState {
    pub switch_device: SwitchDeviceState,
    pub switch_manager: SharedSwitchManager,
}

pub type SharedSwitchAppState = Arc<SwitchAppState>;

// ============================================================================
// Switch-specific properties
// ============================================================================

/// GET /api/v1/switch/{device}/maxswitch
/// Returns the number of switch devices managed by this driver
pub async fn get_maxswitch(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedSwitchAppState>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<i32>::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    let switch_manager = state.switch_manager.read().await;
    let count = switch_manager.get_switch_count() as i32;

    Json(AlpacaResponse::success(
        count,
        params.client_transaction_i_d,
        server_id,
    ))
    .into_response()
}

/// GET /api/v1/switch/{device}/canwrite
/// Returns true if the specified switch can be written (always false for us - read-only)
pub async fn get_canwrite(
    Path((device, switch_id)): Path<(u32, i16)>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedSwitchAppState>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<bool>::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    if switch_id < 0 {
        return Json(AlpacaResponse::<bool>::error(
            params.client_transaction_i_d,
            server_id,
            0x401,
            "Invalid switch ID".to_string(),
        ))
        .into_response();
    }

    let switch_manager = state.switch_manager.read().await;
    let count = switch_manager.get_switch_count();

    if (switch_id as usize) >= count {
        return Json(AlpacaResponse::<bool>::error(
            params.client_transaction_i_d,
            server_id,
            0x401,
            format!("Switch ID {} out of range (max: {})", switch_id, count.saturating_sub(1)),
        ))
        .into_response();
    }

    // All our switches are read-only
    Json(AlpacaResponse::success(
        false,
        params.client_transaction_i_d,
        server_id,
    ))
    .into_response()
}

/// GET /api/v1/switch/{device}/getswitch
/// Returns the state of switch device as boolean
/// Note: true = triggered (unsafe condition), false = not triggered (safe condition)
pub async fn get_switch(
    Path((device, switch_id)): Path<(u32, i16)>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedSwitchAppState>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<bool>::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    if !state.switch_device.is_connected() {
        return Json(AlpacaResponse::<bool>::error(
            params.client_transaction_i_d,
            server_id,
            0x407,
            "Device not connected".to_string(),
        ))
        .into_response();
    }

    if switch_id < 0 {
        return Json(AlpacaResponse::<bool>::error(
            params.client_transaction_i_d,
            server_id,
            0x401,
            "Invalid switch ID".to_string(),
        ))
        .into_response();
    }

    let mut switch_manager = state.switch_manager.write().await;

    match switch_manager.get_switch_by_index(switch_id as usize).await {
        Some(status) => {
            // Return is_triggered state (true = triggered/unsafe condition detected)
            Json(AlpacaResponse::success(
                status.is_triggered,
                params.client_transaction_i_d,
                server_id,
            ))
            .into_response()
        }
        None => Json(AlpacaResponse::<bool>::error(
            params.client_transaction_i_d,
            server_id,
            0x401,
            format!(
                "Switch ID {} out of range (max: {})",
                switch_id,
                switch_manager.get_switch_count().saturating_sub(1)
            ),
        ))
        .into_response(),
    }
}

/// GET /api/v1/switch/{device}/getswitchname
/// Returns the name of the specified switch device
pub async fn get_switchname(
    Path((device, switch_id)): Path<(u32, i16)>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedSwitchAppState>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<String>::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    if switch_id < 0 {
        return Json(AlpacaResponse::<String>::error(
            params.client_transaction_i_d,
            server_id,
            0x401,
            "Invalid switch ID".to_string(),
        ))
        .into_response();
    }

    let mut switch_manager = state.switch_manager.write().await;

    match switch_manager.get_switch_by_index(switch_id as usize).await {
        Some(status) => {
            Json(AlpacaResponse::success(
                status.name,
                params.client_transaction_i_d,
                server_id,
            ))
            .into_response()
        }
        None => Json(AlpacaResponse::<String>::error(
            params.client_transaction_i_d,
            server_id,
            0x401,
            format!(
                "Switch ID {} out of range (max: {})",
                switch_id,
                switch_manager.get_switch_count().saturating_sub(1)
            ),
        ))
        .into_response(),
    }
}

/// GET /api/v1/switch/{device}/getswitchdescription
/// Returns the description of the specified switch device
pub async fn get_switchdescription(
    Path((device, switch_id)): Path<(u32, i16)>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedSwitchAppState>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<String>::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    if switch_id < 0 {
        return Json(AlpacaResponse::<String>::error(
            params.client_transaction_i_d,
            server_id,
            0x401,
            "Invalid switch ID".to_string(),
        ))
        .into_response();
    }

    let mut switch_manager = state.switch_manager.write().await;

    match switch_manager.get_switch_by_index(switch_id as usize).await {
        Some(status) => {
            let description = status.description.unwrap_or_else(|| {
                format!(
                    "Aggregates {} sources using {} logic (hold time: {}s)",
                    status.source_states.len(),
                    status.logic.description(),
                    status.hold_time_seconds
                )
            });
            Json(AlpacaResponse::success(
                description,
                params.client_transaction_i_d,
                server_id,
            ))
            .into_response()
        }
        None => Json(AlpacaResponse::<String>::error(
            params.client_transaction_i_d,
            server_id,
            0x401,
            format!(
                "Switch ID {} out of range (max: {})",
                switch_id,
                switch_manager.get_switch_count().saturating_sub(1)
            ),
        ))
        .into_response(),
    }
}

/// GET /api/v1/switch/{device}/getswitchvalue
/// Returns the value of the specified switch device as a double
/// 1.0 = triggered (unsafe condition), 0.0 = not triggered (safe condition)
pub async fn get_switchvalue(
    Path((device, switch_id)): Path<(u32, i16)>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedSwitchAppState>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<f64>::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    if !state.switch_device.is_connected() {
        return Json(AlpacaResponse::<f64>::error(
            params.client_transaction_i_d,
            server_id,
            0x407,
            "Device not connected".to_string(),
        ))
        .into_response();
    }

    if switch_id < 0 {
        return Json(AlpacaResponse::<f64>::error(
            params.client_transaction_i_d,
            server_id,
            0x401,
            "Invalid switch ID".to_string(),
        ))
        .into_response();
    }

    let mut switch_manager = state.switch_manager.write().await;

    match switch_manager.get_switch_by_index(switch_id as usize).await {
        Some(status) => {
            // Return 1.0 for triggered, 0.0 for not triggered
            let value = if status.is_triggered { 1.0 } else { 0.0 };
            Json(AlpacaResponse::success(
                value,
                params.client_transaction_i_d,
                server_id,
            ))
            .into_response()
        }
        None => Json(AlpacaResponse::<f64>::error(
            params.client_transaction_i_d,
            server_id,
            0x401,
            format!(
                "Switch ID {} out of range (max: {})",
                switch_id,
                switch_manager.get_switch_count().saturating_sub(1)
            ),
        ))
        .into_response(),
    }
}

/// GET /api/v1/switch/{device}/minswitchvalue
/// Returns the minimum value of the specified switch device (always 0.0 for boolean)
pub async fn get_minswitchvalue(
    Path((device, switch_id)): Path<(u32, i16)>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedSwitchAppState>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<f64>::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    if switch_id < 0 {
        return Json(AlpacaResponse::<f64>::error(
            params.client_transaction_i_d,
            server_id,
            0x401,
            "Invalid switch ID".to_string(),
        ))
        .into_response();
    }

    let switch_manager = state.switch_manager.read().await;
    let count = switch_manager.get_switch_count();

    if (switch_id as usize) >= count {
        return Json(AlpacaResponse::<f64>::error(
            params.client_transaction_i_d,
            server_id,
            0x401,
            format!("Switch ID {} out of range (max: {})", switch_id, count.saturating_sub(1)),
        ))
        .into_response();
    }

    Json(AlpacaResponse::success(
        0.0,
        params.client_transaction_i_d,
        server_id,
    ))
    .into_response()
}

/// GET /api/v1/switch/{device}/maxswitchvalue
/// Returns the maximum value of the specified switch device (always 1.0 for boolean)
pub async fn get_maxswitchvalue(
    Path((device, switch_id)): Path<(u32, i16)>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedSwitchAppState>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<f64>::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    if switch_id < 0 {
        return Json(AlpacaResponse::<f64>::error(
            params.client_transaction_i_d,
            server_id,
            0x401,
            "Invalid switch ID".to_string(),
        ))
        .into_response();
    }

    let switch_manager = state.switch_manager.read().await;
    let count = switch_manager.get_switch_count();

    if (switch_id as usize) >= count {
        return Json(AlpacaResponse::<f64>::error(
            params.client_transaction_i_d,
            server_id,
            0x401,
            format!("Switch ID {} out of range (max: {})", switch_id, count.saturating_sub(1)),
        ))
        .into_response();
    }

    Json(AlpacaResponse::success(
        1.0,
        params.client_transaction_i_d,
        server_id,
    ))
    .into_response()
}

/// GET /api/v1/switch/{device}/switchstep
/// Returns the step size for the specified switch device (always 1.0 for boolean)
pub async fn get_switchstep(
    Path((device, switch_id)): Path<(u32, i16)>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedSwitchAppState>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<f64>::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    if switch_id < 0 {
        return Json(AlpacaResponse::<f64>::error(
            params.client_transaction_i_d,
            server_id,
            0x401,
            "Invalid switch ID".to_string(),
        ))
        .into_response();
    }

    let switch_manager = state.switch_manager.read().await;
    let count = switch_manager.get_switch_count();

    if (switch_id as usize) >= count {
        return Json(AlpacaResponse::<f64>::error(
            params.client_transaction_i_d,
            server_id,
            0x401,
            format!("Switch ID {} out of range (max: {})", switch_id, count.saturating_sub(1)),
        ))
        .into_response();
    }

    Json(AlpacaResponse::success(
        1.0,
        params.client_transaction_i_d,
        server_id,
    ))
    .into_response()
}

// ============================================================================
// Write operations (all return errors since we're read-only)
// ============================================================================

/// PUT request for SetSwitch
#[derive(Debug, serde::Deserialize)]
pub struct SetSwitchRequest {
    #[serde(default, alias = "ClientID", alias = "clientid")]
    #[serde(rename = "ClientID")]
    pub client_i_d: u32,
    #[serde(default, alias = "ClientTransactionID", alias = "clienttransactionid")]
    #[serde(rename = "ClientTransactionID")]
    pub client_transaction_i_d: u32,
    #[serde(alias = "State", alias = "state")]
    #[serde(rename = "State")]
    pub state: String,
}

/// PUT request for SetSwitchValue
#[derive(Debug, serde::Deserialize)]
pub struct SetSwitchValueRequest {
    #[serde(default, alias = "ClientID", alias = "clientid")]
    #[serde(rename = "ClientID")]
    pub client_i_d: u32,
    #[serde(default, alias = "ClientTransactionID", alias = "clienttransactionid")]
    #[serde(rename = "ClientTransactionID")]
    pub client_transaction_i_d: u32,
    #[serde(alias = "Value", alias = "value")]
    #[serde(rename = "Value")]
    pub value: f64,
}

/// PUT request for SetSwitchName
#[derive(Debug, serde::Deserialize)]
pub struct SetSwitchNameRequest {
    #[serde(default, alias = "ClientID", alias = "clientid")]
    #[serde(rename = "ClientID")]
    pub client_i_d: u32,
    #[serde(default, alias = "ClientTransactionID", alias = "clienttransactionid")]
    #[serde(rename = "ClientTransactionID")]
    pub client_transaction_i_d: u32,
    #[serde(alias = "Name", alias = "name")]
    #[serde(rename = "Name")]
    pub name: String,
}

/// PUT /api/v1/switch/{device}/setswitch
pub async fn put_setswitch(
    Path((device, _switch_id)): Path<(u32, i16)>,
    State(state): State<SharedSwitchAppState>,
    AlpacaForm(request): AlpacaForm<SetSwitchRequest>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<()>::error(
            request.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    // All switches are read-only
    Json(AlpacaResponse::<()>::error(
        request.client_transaction_i_d,
        server_id,
        0x40B, // PropertyNotImplementedException
        "This switch is read-only and cannot be set".to_string(),
    ))
    .into_response()
}

/// PUT /api/v1/switch/{device}/setswitchvalue
pub async fn put_setswitchvalue(
    Path((device, _switch_id)): Path<(u32, i16)>,
    State(state): State<SharedSwitchAppState>,
    AlpacaForm(request): AlpacaForm<SetSwitchValueRequest>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<()>::error(
            request.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    // All switches are read-only
    Json(AlpacaResponse::<()>::error(
        request.client_transaction_i_d,
        server_id,
        0x40B, // PropertyNotImplementedException
        "This switch is read-only and cannot be set".to_string(),
    ))
    .into_response()
}

/// PUT /api/v1/switch/{device}/setswitchname
pub async fn put_setswitchname(
    Path((device, _switch_id)): Path<(u32, i16)>,
    State(state): State<SharedSwitchAppState>,
    AlpacaForm(request): AlpacaForm<SetSwitchNameRequest>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<()>::error(
            request.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    // All switches are read-only
    Json(AlpacaResponse::<()>::error(
        request.client_transaction_i_d,
        server_id,
        0x40B, // PropertyNotImplementedException
        "Switch names are read-only and cannot be set".to_string(),
    ))
    .into_response()
}

// ============================================================================
// Common ASCOM device properties
// ============================================================================

/// GET /api/v1/switch/{device}/connected
pub async fn get_connected(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedSwitchAppState>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<bool>::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    Json(AlpacaResponse::success(
        state.switch_device.is_connected(),
        params.client_transaction_i_d,
        server_id,
    ))
    .into_response()
}

/// PUT /api/v1/switch/{device}/connected
pub async fn put_connected(
    Path(device): Path<u32>,
    State(state): State<SharedSwitchAppState>,
    AlpacaForm(request): AlpacaForm<ConnectedRequest>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<()>::error(
            request.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    *state.switch_device.connected.write() = request.connected;

    Json(AlpacaResponse::success(
        (),
        request.client_transaction_i_d,
        server_id,
    ))
    .into_response()
}

/// GET /api/v1/switch/{device}/name
pub async fn get_name(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedSwitchAppState>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<String>::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    Json(AlpacaResponse::success(
        state.switch_device.device_name.clone(),
        params.client_transaction_i_d,
        server_id,
    ))
    .into_response()
}

/// GET /api/v1/switch/{device}/description
pub async fn get_description(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedSwitchAppState>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<String>::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    Json(AlpacaResponse::success(
        state.switch_device.description.clone(),
        params.client_transaction_i_d,
        server_id,
    ))
    .into_response()
}

/// GET /api/v1/switch/{device}/driverinfo
pub async fn get_driverinfo(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedSwitchAppState>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<String>::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    Json(AlpacaResponse::success(
        state.switch_device.driver_info.clone(),
        params.client_transaction_i_d,
        server_id,
    ))
    .into_response()
}

/// GET /api/v1/switch/{device}/driverversion
pub async fn get_driverversion(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedSwitchAppState>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<String>::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    Json(AlpacaResponse::success(
        state.switch_device.driver_version.clone(),
        params.client_transaction_i_d,
        server_id,
    ))
    .into_response()
}

/// GET /api/v1/switch/{device}/interfaceversion
pub async fn get_interfaceversion(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedSwitchAppState>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<i32>::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    // ISwitchV2 interface version
    Json(AlpacaResponse::success(
        2,
        params.client_transaction_i_d,
        server_id,
    ))
    .into_response()
}

/// GET /api/v1/switch/{device}/supportedactions
pub async fn get_supportedactions(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedSwitchAppState>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<Vec<String>>::error(
            params.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    Json(AlpacaResponse::success(
        Vec::<String>::new(),
        params.client_transaction_i_d,
        server_id,
    ))
    .into_response()
}

/// PUT /api/v1/switch/{device}/action
pub async fn put_action(
    Path(device): Path<u32>,
    State(state): State<SharedSwitchAppState>,
    AlpacaForm(request): AlpacaForm<ActionRequest>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<String>::error(
            request.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    if !state.switch_device.is_connected() {
        return Json(AlpacaResponse::<String>::error(
            request.client_transaction_i_d,
            server_id,
            0x407,
            "Device not connected".to_string(),
        ))
        .into_response();
    }

    Json(AlpacaResponse::<String>::error(
        request.client_transaction_i_d,
        server_id,
        0x40C,
        format!("Action '{}' is not supported", request.action),
    ))
    .into_response()
}

/// PUT /api/v1/switch/{device}/commandblind
pub async fn put_commandblind(
    Path(device): Path<u32>,
    State(state): State<SharedSwitchAppState>,
    AlpacaForm(request): AlpacaForm<CommandRequest>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<()>::error(
            request.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    if !state.switch_device.is_connected() {
        return Json(AlpacaResponse::<()>::error(
            request.client_transaction_i_d,
            server_id,
            0x407,
            "Device not connected".to_string(),
        ))
        .into_response();
    }

    Json(AlpacaResponse::<()>::error(
        request.client_transaction_i_d,
        server_id,
        0x40C,
        "CommandBlind is not implemented".to_string(),
    ))
    .into_response()
}

/// PUT /api/v1/switch/{device}/commandbool
pub async fn put_commandbool(
    Path(device): Path<u32>,
    State(state): State<SharedSwitchAppState>,
    AlpacaForm(request): AlpacaForm<CommandRequest>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<bool>::error(
            request.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    if !state.switch_device.is_connected() {
        return Json(AlpacaResponse::<bool>::error(
            request.client_transaction_i_d,
            server_id,
            0x407,
            "Device not connected".to_string(),
        ))
        .into_response();
    }

    Json(AlpacaResponse::<bool>::error(
        request.client_transaction_i_d,
        server_id,
        0x40C,
        "CommandBool is not implemented".to_string(),
    ))
    .into_response()
}

/// PUT /api/v1/switch/{device}/commandstring
pub async fn put_commandstring(
    Path(device): Path<u32>,
    State(state): State<SharedSwitchAppState>,
    AlpacaForm(request): AlpacaForm<CommandRequest>,
) -> Response {
    let server_id = state.switch_device.next_transaction_id();

    if device != 0 {
        return Json(AlpacaResponse::<String>::error(
            request.client_transaction_i_d,
            server_id,
            0x400,
            "Invalid device number".to_string(),
        ))
        .into_response();
    }

    if !state.switch_device.is_connected() {
        return Json(AlpacaResponse::<String>::error(
            request.client_transaction_i_d,
            server_id,
            0x407,
            "Device not connected".to_string(),
        ))
        .into_response();
    }

    Json(AlpacaResponse::<String>::error(
        request.client_transaction_i_d,
        server_id,
        0x40C,
        "CommandString is not implemented".to_string(),
    ))
    .into_response()
}
