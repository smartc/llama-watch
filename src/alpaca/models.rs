use serde::{Deserialize, Deserializer, Serialize};

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

/// PUT request for connected property
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ConnectedRequest {
    #[serde(default)]
    pub client_i_d: u32,
    #[serde(default)]
    pub client_transaction_i_d: u32,
    #[serde(deserialize_with = "deserialize_bool_from_string")]
    pub connected: bool,
}

/// Standard query parameters
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct QueryParams {
    #[serde(default)]
    pub client_i_d: u32,
    #[serde(default)]
    pub client_transaction_i_d: u32,
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
