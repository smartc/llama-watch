use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Json, IntoResponse, Response},
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::models::{AlpacaForm, AlpacaResponse, ConnectedRequest, QueryParams};
use crate::weather::observing_conditions::{
    ObservingConditionsDevice, WeatherDataAccessor, ObservingConditionsProperties,
    get_sensor_description, is_property_supported, get_property_value,
};

pub type ObservingConditionsState = Arc<RwLock<HashMap<u32, Arc<ObservingConditionsDevice>>>>;

pub struct ObservingConditionsAppState {
    pub devices: ObservingConditionsState,
    pub weather_data: Arc<WeatherDataAccessor>,
}

pub type SharedObservingConditionsState = Arc<ObservingConditionsAppState>;

// Common error codes
const ERROR_NOT_CONNECTED: i32 = 0x407;
const ERROR_INVALID_VALUE: i32 = 0x401;
const ERROR_NOT_IMPLEMENTED: i32 = 0x400;
const ERROR_INVALID_OPERATION: i32 = 0x40B;

// Helper to get device or return error
async fn get_device(
    devices: &ObservingConditionsState,
    device_number: u32,
) -> Result<Arc<ObservingConditionsDevice>, (StatusCode, String)> {
    let devices_map = devices.read().await;
    devices_map.get(&device_number)
        .cloned()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, format!("Invalid device number: {}", device_number)))
}

// GET /api/v1/observingconditions/{device}/connected
pub async fn get_connected(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedObservingConditionsState>,
) -> Response {
    let dev = match get_device(&state.devices, device).await {
        Ok(d) => d,
        Err((status, msg)) => {
            return (status, msg).into_response();
        }
    };

    let server_id = dev.next_transaction_id();
    let connected = dev.is_connected();

    Json(AlpacaResponse::success(
        connected,
        params.client_transaction_i_d,
        server_id,
    )).into_response()
}

// PUT /api/v1/observingconditions/{device}/connected
pub async fn put_connected(
    Path(device): Path<u32>,
    State(state): State<SharedObservingConditionsState>,
    AlpacaForm(req): AlpacaForm<ConnectedRequest>,
) -> Response {
    let dev = match get_device(&state.devices, device).await {
        Ok(d) => d,
        Err((status, msg)) => {
            return (status, msg).into_response();
        }
    };

    let server_id = dev.next_transaction_id();
    dev.set_connected(req.connected);

    Json(AlpacaResponse::success(
        (),
        req.client_transaction_i_d,
        server_id,
    )).into_response()
}

// Macro to reduce boilerplate for property getters
macro_rules! property_getter {
    ($func_name:ident, $property:expr, $return_type:ty) => {
        pub async fn $func_name(
            Path(device): Path<u32>,
            Query(params): Query<QueryParams>,
            State(state): State<SharedObservingConditionsState>,
        ) -> Response {
            let dev = match get_device(&state.devices, device).await {
                Ok(d) => d,
                Err((status, msg)) => {
                    return (status, msg).into_response();
                }
            };

            let server_id = dev.next_transaction_id();

            if !dev.is_connected() {
                return Json(AlpacaResponse::<$return_type>::error(
                    params.client_transaction_i_d,
                    server_id,
                    ERROR_NOT_CONNECTED,
                    "Device is not connected".to_string(),
                )).into_response();
            }

            // Check if property is supported
            if !is_property_supported($property) {
                return Json(AlpacaResponse::<$return_type>::error(
                    params.client_transaction_i_d,
                    server_id,
                    ERROR_NOT_IMPLEMENTED,
                    format!("Property '{}' is not implemented", $property),
                )).into_response();
            }

            // Get observation data
            let config = dev.get_config();
            let obs = match state.weather_data.get_observation(&config).await {
                Some(o) => o,
                None => {
                    return Json(AlpacaResponse::<$return_type>::error(
                        params.client_transaction_i_d,
                        server_id,
                        ERROR_INVALID_OPERATION,
                        "No weather data available".to_string(),
                    )).into_response();
                }
            };

            let prev_obs = state.weather_data.get_previous_observation(&config).await;
            let value = match get_property_value(&obs, $property, prev_obs.as_ref()) {
                Some(v) => v,
                None => {
                    return Json(AlpacaResponse::<$return_type>::error(
                        params.client_transaction_i_d,
                        server_id,
                        ERROR_NOT_IMPLEMENTED,
                        format!("Property '{}' is not available", $property),
                    )).into_response();
                }
            };

            Json(AlpacaResponse::success(
                value as $return_type,
                params.client_transaction_i_d,
                server_id,
            )).into_response()
        }
    };
}

// Define all property getters
property_getter!(get_dewpoint, ObservingConditionsProperties::DEW_POINT, f64);
property_getter!(get_humidity, ObservingConditionsProperties::HUMIDITY, f64);
property_getter!(get_pressure, ObservingConditionsProperties::PRESSURE, f64);
property_getter!(get_rainrate, ObservingConditionsProperties::RAIN_RATE, f64);
property_getter!(get_skybrightness, ObservingConditionsProperties::SKY_BRIGHTNESS, f64);
property_getter!(get_temperature, ObservingConditionsProperties::TEMPERATURE, f64);
property_getter!(get_winddirection, ObservingConditionsProperties::WIND_DIRECTION, f64);
property_getter!(get_windgust, ObservingConditionsProperties::WIND_GUST, f64);
property_getter!(get_windspeed, ObservingConditionsProperties::WIND_SPEED, f64);

// Properties that are not implemented (will return error)
pub async fn get_cloudcover(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedObservingConditionsState>,
) -> Response {
    let dev = match get_device(&state.devices, device).await {
        Ok(d) => d,
        Err((status, msg)) => {
            return (status, msg).into_response();
        }
    };

    let server_id = dev.next_transaction_id();
    Json(AlpacaResponse::<f64>::error(
        params.client_transaction_i_d,
        server_id,
        ERROR_NOT_IMPLEMENTED,
        "CloudCover is not implemented".to_string(),
    )).into_response()
}

pub async fn get_skyquality(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedObservingConditionsState>,
) -> Response {
    let dev = match get_device(&state.devices, device).await {
        Ok(d) => d,
        Err((status, msg)) => {
            return (status, msg).into_response();
        }
    };

    let server_id = dev.next_transaction_id();
    Json(AlpacaResponse::<f64>::error(
        params.client_transaction_i_d,
        server_id,
        ERROR_NOT_IMPLEMENTED,
        "SkyQuality is not implemented".to_string(),
    )).into_response()
}

pub async fn get_skytemperature(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedObservingConditionsState>,
) -> Response {
    let dev = match get_device(&state.devices, device).await {
        Ok(d) => d,
        Err((status, msg)) => {
            return (status, msg).into_response();
        }
    };

    let server_id = dev.next_transaction_id();
    Json(AlpacaResponse::<f64>::error(
        params.client_transaction_i_d,
        server_id,
        ERROR_NOT_IMPLEMENTED,
        "SkyTemperature is not implemented".to_string(),
    )).into_response()
}

pub async fn get_starfwhm(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedObservingConditionsState>,
) -> Response {
    let dev = match get_device(&state.devices, device).await {
        Ok(d) => d,
        Err((status, msg)) => {
            return (status, msg).into_response();
        }
    };

    let server_id = dev.next_transaction_id();
    Json(AlpacaResponse::<f64>::error(
        params.client_transaction_i_d,
        server_id,
        ERROR_NOT_IMPLEMENTED,
        "StarFWHM is not implemented".to_string(),
    )).into_response()
}

// GET /api/v1/observingconditions/{device}/averageperiod
pub async fn get_averageperiod(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedObservingConditionsState>,
) -> Response {
    let dev = match get_device(&state.devices, device).await {
        Ok(d) => d,
        Err((status, msg)) => {
            return (status, msg).into_response();
        }
    };

    let server_id = dev.next_transaction_id();

    // Return 0.0 as we use instantaneous values
    Json(AlpacaResponse::success(
        0.0,
        params.client_transaction_i_d,
        server_id,
    )).into_response()
}

// PUT /api/v1/observingconditions/{device}/averageperiod
#[derive(serde::Deserialize)]
pub struct AveragePeriodRequest {
    #[serde(default)]
    #[serde(rename = "ClientID")]
    pub client_i_d: u32,
    #[serde(default)]
    #[serde(rename = "ClientTransactionID")]
    pub client_transaction_i_d: u32,
    #[serde(rename = "AveragePeriod")]
    pub average_period: f64,
}

pub async fn put_averageperiod(
    Path(device): Path<u32>,
    State(state): State<SharedObservingConditionsState>,
    AlpacaForm(req): AlpacaForm<AveragePeriodRequest>,
) -> Response {
    let dev = match get_device(&state.devices, device).await {
        Ok(d) => d,
        Err((status, msg)) => {
            return (status, msg).into_response();
        }
    };

    let server_id = dev.next_transaction_id();

    // We don't actually support averaging, but accept the value to be compliant
    if req.average_period < 0.0 {
        return Json(AlpacaResponse::<()>::error(
            req.client_transaction_i_d,
            server_id,
            ERROR_INVALID_VALUE,
            "AveragePeriod cannot be negative".to_string(),
        )).into_response();
    }

    Json(AlpacaResponse::success(
        (),
        req.client_transaction_i_d,
        server_id,
    )).into_response()
}

// GET /api/v1/observingconditions/{device}/sensordescription
#[derive(serde::Deserialize)]
pub struct SensorDescriptionQuery {
    #[serde(default)]
    #[serde(rename = "ClientID")]
    pub client_i_d: u32,
    #[serde(default)]
    #[serde(rename = "ClientTransactionID")]
    pub client_transaction_i_d: u32,
    #[serde(rename = "SensorName")]
    pub sensor_name: String,
}

pub async fn get_sensordescription(
    Path(device): Path<u32>,
    Query(params): Query<SensorDescriptionQuery>,
    State(state): State<SharedObservingConditionsState>,
) -> Response {
    let dev = match get_device(&state.devices, device).await {
        Ok(d) => d,
        Err((status, msg)) => {
            return (status, msg).into_response();
        }
    };

    let server_id = dev.next_transaction_id();
    let description = get_sensor_description(&params.sensor_name);

    Json(AlpacaResponse::success(
        description,
        params.client_transaction_i_d,
        server_id,
    )).into_response()
}

// GET /api/v1/observingconditions/{device}/timesincelastupdate
#[derive(serde::Deserialize)]
pub struct TimeSinceQuery {
    #[serde(default)]
    #[serde(rename = "ClientID")]
    pub client_i_d: u32,
    #[serde(default)]
    #[serde(rename = "ClientTransactionID")]
    pub client_transaction_i_d: u32,
    #[serde(rename = "SensorName")]
    pub sensor_name: String,
}

pub async fn get_timesincelastupdate(
    Path(device): Path<u32>,
    Query(params): Query<TimeSinceQuery>,
    State(state): State<SharedObservingConditionsState>,
) -> Response {
    let dev = match get_device(&state.devices, device).await {
        Ok(d) => d,
        Err((status, msg)) => {
            return (status, msg).into_response();
        }
    };

    let server_id = dev.next_transaction_id();

    if !dev.is_connected() {
        return Json(AlpacaResponse::<f64>::error(
            params.client_transaction_i_d,
            server_id,
            ERROR_NOT_CONNECTED,
            "Device is not connected".to_string(),
        )).into_response();
    }

    let config = dev.get_config();
    let time_since = match state.weather_data.time_since_last_update(&config, &params.sensor_name).await {
        Some(t) => t,
        None => {
            return Json(AlpacaResponse::<f64>::error(
                params.client_transaction_i_d,
                server_id,
                ERROR_INVALID_OPERATION,
                "No weather data available".to_string(),
            )).into_response();
        }
    };

    Json(AlpacaResponse::success(
        time_since,
        params.client_transaction_i_d,
        server_id,
    )).into_response()
}

// PUT /api/v1/observingconditions/{device}/refresh
pub async fn put_refresh(
    Path(device): Path<u32>,
    State(state): State<SharedObservingConditionsState>,
    AlpacaForm(req): AlpacaForm<ConnectedRequest>,
) -> Response {
    let dev = match get_device(&state.devices, device).await {
        Ok(d) => d,
        Err((status, msg)) => {
            return (status, msg).into_response();
        }
    };

    let server_id = dev.next_transaction_id();

    // Refresh is a no-op for us since we continuously receive UDP data
    Json(AlpacaResponse::success(
        (),
        req.client_transaction_i_d,
        server_id,
    )).into_response()
}

// Common property endpoints (name, description, etc.)
pub async fn get_name(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedObservingConditionsState>,
) -> Response {
    let dev = match get_device(&state.devices, device).await {
        Ok(d) => d,
        Err((status, msg)) => {
            return (status, msg).into_response();
        }
    };

    let server_id = dev.next_transaction_id();
    Json(AlpacaResponse::success(
        dev.name.clone(),
        params.client_transaction_i_d,
        server_id,
    )).into_response()
}

pub async fn get_description(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedObservingConditionsState>,
) -> Response {
    let dev = match get_device(&state.devices, device).await {
        Ok(d) => d,
        Err((status, msg)) => {
            return (status, msg).into_response();
        }
    };

    let server_id = dev.next_transaction_id();
    Json(AlpacaResponse::success(
        dev.description.clone(),
        params.client_transaction_i_d,
        server_id,
    )).into_response()
}

pub async fn get_driverinfo(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedObservingConditionsState>,
) -> Response {
    let dev = match get_device(&state.devices, device).await {
        Ok(d) => d,
        Err((status, msg)) => {
            return (status, msg).into_response();
        }
    };

    let server_id = dev.next_transaction_id();
    Json(AlpacaResponse::success(
        dev.driver_info.clone(),
        params.client_transaction_i_d,
        server_id,
    )).into_response()
}

pub async fn get_driverversion(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedObservingConditionsState>,
) -> Response {
    let dev = match get_device(&state.devices, device).await {
        Ok(d) => d,
        Err((status, msg)) => {
            return (status, msg).into_response();
        }
    };

    let server_id = dev.next_transaction_id();
    Json(AlpacaResponse::success(
        dev.driver_version.clone(),
        params.client_transaction_i_d,
        server_id,
    )).into_response()
}

pub async fn get_interfaceversion(
    Path(_device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedObservingConditionsState>,
) -> Response {
    // Interface version is always 1 for all devices
    let devices_map = state.devices.read().await;
    if let Some(dev) = devices_map.values().next() {
        let server_id = dev.next_transaction_id();
        return Json(AlpacaResponse::success(
            1i32,
            params.client_transaction_i_d,
            server_id,
        )).into_response();
    }

    Json(AlpacaResponse::success(
        1i32,
        params.client_transaction_i_d,
        0,
    )).into_response()
}

pub async fn get_supportedactions(
    Path(device): Path<u32>,
    Query(params): Query<QueryParams>,
    State(state): State<SharedObservingConditionsState>,
) -> Response {
    let dev = match get_device(&state.devices, device).await {
        Ok(d) => d,
        Err((status, msg)) => {
            return (status, msg).into_response();
        }
    };

    let server_id = dev.next_transaction_id();
    let empty: Vec<String> = vec![];
    Json(AlpacaResponse::success(
        empty,
        params.client_transaction_i_d,
        server_id,
    )).into_response()
}
