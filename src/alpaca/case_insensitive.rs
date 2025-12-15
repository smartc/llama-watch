use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
};
use serde::de::DeserializeOwned;
use std::collections::HashMap;

/// Case-insensitive query parameter extractor for ASCOM Alpaca compliance
pub struct CaseInsensitiveQuery<T>(pub T);

#[async_trait]
impl<T, S> FromRequestParts<S> for CaseInsensitiveQuery<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let query = parts.uri.query().unwrap_or_default();

        // Parse query string into case-insensitive map
        let params: HashMap<String, String> = serde_urlencoded::from_str(query)
            .map_err(|err| {
                (StatusCode::BAD_REQUEST, format!("Failed to deserialize query string: {}", err))
                    .into_response()
            })?;

        // Create case-insensitive version by normalizing keys
        let ci_params: HashMap<String, String> = params
            .into_iter()
            .map(|(k, v)| (normalize_key(&k), v))
            .collect();

        // Serialize back to query string with normalized keys
        let normalized_query = serde_urlencoded::to_string(&ci_params)
            .map_err(|err| {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to serialize query: {}", err))
                    .into_response()
            })?;

        // Deserialize into target type
        serde_urlencoded::from_str(&normalized_query)
            .map(CaseInsensitiveQuery)
            .map_err(|err| {
                (StatusCode::BAD_REQUEST, format!("Failed to deserialize query string: {}", err))
                    .into_response()
            })
    }
}

/// Normalize parameter names to standard ASCOM casing (case-insensitive matching)
fn normalize_key(key: &str) -> String {
    let lower = key.to_lowercase();
    match lower.as_str() {
        "clientid" => "ClientID".to_string(),
        "clienttransactionid" => "ClientTransactionID".to_string(),
        "sensorname" => "SensorName".to_string(),
        "averageperiod" => "AveragePeriod".to_string(),
        "connected" => "Connected".to_string(),
        _ => key.to_string(), // Pass through unknown keys
    }
}
