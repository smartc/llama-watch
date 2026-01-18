use axum::{
    async_trait,
    extract::{FromRequest, Request, rejection::FormRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    Form,
};
use serde::{Deserialize, Deserializer, Serialize};

/// Custom Form extractor that returns HTTP 400 instead of 422 for validation errors
pub struct AlpacaForm<T>(pub T);

#[async_trait]
impl<S, T> FromRequest<S> for AlpacaForm<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
    Form<T>: FromRequest<S, Rejection = FormRejection>,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Form::<T>::from_request(req, state).await {
            Ok(Form(value)) => Ok(AlpacaForm(value)),
            Err(rejection) => {
                let message = rejection.body_text();
                Err((StatusCode::BAD_REQUEST, message).into_response())
            }
        }
    }
}

/// Deserialize a boolean from various string representations (case-insensitive)
/// Handles "true", "false", "True", "False", "TRUE", "FALSE", "1", "0"
fn deserialize_bool_from_string<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.to_lowercase().as_str() {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(serde::de::Error::custom(format!(
            "expected boolean string, got '{}'",
            s
        ))),
    }
}

/// Non-standard structured comments for safety status details
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SafetyComments {
    pub unsafe_monitors: Vec<MonitorComment>,
}

/// Details about an unsafe monitor
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MonitorComment {
    pub monitor: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    pub threshold: f64,
}

/// Standard ASCOM Alpaca response wrapper
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AlpacaResponse<T> {
    pub value: T,
    pub client_transaction_i_d: u32,
    pub server_transaction_i_d: u32,
    pub error_number: i32,
    pub error_message: String,
    /// Non-standard field for additional information (e.g., safety status details)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comments: Option<SafetyComments>,
}

impl<T: Default> AlpacaResponse<T> {
    pub fn success(value: T, client_id: u32, server_id: u32) -> Self {
        Self {
            value,
            client_transaction_i_d: client_id,
            server_transaction_i_d: server_id,
            error_number: 0,
            error_message: String::new(),
            comments: None,
        }
    }

    pub fn success_with_comments(value: T, client_id: u32, server_id: u32, comments: SafetyComments) -> Self {
        Self {
            value,
            client_transaction_i_d: client_id,
            server_transaction_i_d: server_id,
            error_number: 0,
            error_message: String::new(),
            comments: Some(comments),
        }
    }

    pub fn error(client_id: u32, server_id: u32, error_number: i32, error_message: String) -> Self {
        Self {
            value: T::default(),
            client_transaction_i_d: client_id,
            server_transaction_i_d: server_id,
            error_number,
            error_message,
            comments: None,
        }
    }
}

/// Standard PUT request form data
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PutRequest {
    #[serde(default)]
    pub client_i_d: u32,
    #[serde(default)]
    pub client_transaction_i_d: u32,
}

/// PUT request for connected property (case-insensitive per ASCOM spec)
#[derive(Debug, Deserialize)]
pub struct ConnectedRequest {
    #[serde(default, alias = "ClientID", alias = "clientid", alias = "CLIENTID", alias = "clientId")]
    #[serde(rename = "ClientID")]
    pub client_i_d: u32,
    #[serde(default, alias = "ClientTransactionID", alias = "clienttransactionid", alias = "CLIENTTRANSACTIONID", alias = "clientTransactionId")]
    #[serde(rename = "ClientTransactionID")]
    pub client_transaction_i_d: u32,
    #[serde(deserialize_with = "deserialize_bool_from_string", alias = "Connected", alias = "connected", alias = "CONNECTED")]
    #[serde(rename = "Connected")]
    pub connected: bool,
}

/// Standard query parameters (case-insensitive per ASCOM spec)
#[derive(Debug, Deserialize)]
pub struct QueryParams {
    #[serde(default, alias = "ClientID", alias = "clientid", alias = "CLIENTID", alias = "clientId")]
    #[serde(rename = "ClientID")]
    pub client_i_d: u32,
    #[serde(default, alias = "ClientTransactionID", alias = "clienttransactionid", alias = "CLIENTTRANSACTIONID", alias = "clientTransactionId")]
    #[serde(rename = "ClientTransactionID")]
    pub client_transaction_i_d: u32,
}

/// Query parameters for switch methods that require an Id parameter
#[derive(Debug, Deserialize)]
pub struct SwitchQueryParams {
    #[serde(default, alias = "ClientID", alias = "clientid", alias = "CLIENTID", alias = "clientId")]
    #[serde(rename = "ClientID")]
    pub client_i_d: u32,
    #[serde(default, alias = "ClientTransactionID", alias = "clienttransactionid", alias = "CLIENTTRANSACTIONID", alias = "clientTransactionId")]
    #[serde(rename = "ClientTransactionID")]
    pub client_transaction_i_d: u32,
    #[serde(alias = "ID", alias = "id", alias = "Id", alias = "iD")]
    #[serde(rename = "Id")]
    pub id: i16,
}

/// PUT request for Action method
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ActionRequest {
    #[serde(default)]
    pub client_i_d: u32,
    #[serde(default)]
    pub client_transaction_i_d: u32,
    pub action: String,
    pub parameters: String,
}

/// PUT request for Command methods
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CommandRequest {
    #[serde(default)]
    pub client_i_d: u32,
    #[serde(default)]
    pub client_transaction_i_d: u32,
    pub command: String,
    #[serde(default)]
    pub raw: bool,
}
