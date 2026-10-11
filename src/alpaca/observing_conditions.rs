use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Json, IntoResponse, Response},
};
use super::case_insensitive::CaseInsensitiveQuery;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::models::{AlpacaForm, AlpacaResponse, ConnectedRequest, QueryParams};
use crate::weather::observation::WeatherProperty;
use crate::weather::observing_conditions::{
    ObservingConditionsDevice, WeatherDataAccessor, get_sensor_description, is_property_supported,
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

            let property: WeatherProperty = $property;
            let name = property.ascom_name().unwrap_or(property.label());

            // Check if property is supported
            let config = dev.get_config();
            if !is_property_supported(&config, property) {
                return Json(AlpacaResponse::<$return_type>::error(
                    params.client_transaction_i_d,
                    server_id,
                    ERROR_NOT_IMPLEMENTED,
                    format!("Property '{}' is not implemented", name),
                )).into_response();
            }

            let readings = state.weather_data.readings(&config).await;
            let value = match readings.get(&property) {
                Some(reading) => reading.value,
                None => {
                    return Json(AlpacaResponse::<$return_type>::error(
                        params.client_transaction_i_d,
                        server_id,
                        ERROR_INVALID_OPERATION,
                        format!("No data available for '{}'", name),
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
property_getter!(get_dewpoint, WeatherProperty::DewPoint, f64);
property_getter!(get_humidity, WeatherProperty::Humidity, f64);
property_getter!(get_pressure, WeatherProperty::Pressure, f64);
property_getter!(get_rainrate, WeatherProperty::RainRate, f64);
property_getter!(get_skybrightness, WeatherProperty::SkyBrightness, f64);
property_getter!(get_temperature, WeatherProperty::Temperature, f64);
property_getter!(get_winddirection, WeatherProperty::WindDirection, f64);
property_getter!(get_windgust, WeatherProperty::WindGust, f64);
property_getter!(get_windspeed, WeatherProperty::WindSpeed, f64);

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
    #[serde(default)]
    #[serde(rename = "SensorName")]
    pub sensor_name: String,
}

pub async fn get_sensordescription(
    Path(device): Path<u32>,
    CaseInsensitiveQuery(params): CaseInsensitiveQuery<SensorDescriptionQuery>,
    State(state): State<SharedObservingConditionsState>,
) -> Response {
    let dev = match get_device(&state.devices, device).await {
        Ok(d) => d,
        Err((status, msg)) => {
            return (status, msg).into_response();
        }
    };

    let server_id = dev.next_transaction_id();

    // Check if property is supported - if not, return NotImplementedException
    let config = dev.get_config();
    let property = match WeatherProperty::from_ascom_name(&params.sensor_name) {
        Some(p) if is_property_supported(&config, p) => p,
        _ => {
            return Json(AlpacaResponse::<String>::error(
                params.client_transaction_i_d,
                server_id,
                ERROR_NOT_IMPLEMENTED,
                format!("Property '{}' is not implemented", params.sensor_name),
            )).into_response();
        }
    };

    let description = get_sensor_description(&config, property);

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
    #[serde(default)]
    #[serde(rename = "SensorName")]
    pub sensor_name: String,
}

pub async fn get_timesincelastupdate(
    Path(device): Path<u32>,
    CaseInsensitiveQuery(params): CaseInsensitiveQuery<TimeSinceQuery>,
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
    let readings = state.weather_data.readings(&config).await;
    let now = chrono::Utc::now();
    let age = |timestamp: chrono::DateTime<chrono::Utc>| {
        now.signed_duration_since(timestamp).num_milliseconds() as f64 / 1000.0
    };

    // Empty string means "time since most recent update of ANY sensor"
    let time_since = if params.sensor_name.is_empty() {
        readings.iter()
            .filter(|(p, _)| p.ascom_name().is_some())
            .map(|(_, r)| r.timestamp)
            .max()
            .map(age)
    } else {
        // Check if property is supported - if not, return NotImplementedException
        let property = match WeatherProperty::from_ascom_name(&params.sensor_name) {
            Some(p) if is_property_supported(&config, p) => p,
            _ => {
                return Json(AlpacaResponse::<f64>::error(
                    params.client_transaction_i_d,
                    server_id,
                    ERROR_NOT_IMPLEMENTED,
                    format!("Property '{}' is not implemented", params.sensor_name),
                )).into_response();
            }
        };
        readings.get(&property).map(|r| age(r.timestamp))
    };

    match time_since {
        Some(t) => Json(AlpacaResponse::success(
            t,
            params.client_transaction_i_d,
            server_id,
        )).into_response(),
        None => Json(AlpacaResponse::<f64>::error(
            params.client_transaction_i_d,
            server_id,
            ERROR_INVALID_OPERATION,
            "No weather data available".to_string(),
        )).into_response(),
    }
}

// PUT /api/v1/observingconditions/{device}/refresh
pub async fn put_refresh(
    Path(device): Path<u32>,
    State(state): State<SharedObservingConditionsState>,
    AlpacaForm(params): AlpacaForm<QueryParams>,
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
        params.client_transaction_i_d,
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
