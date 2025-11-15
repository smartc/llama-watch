use serde::{Deserialize, Serialize};

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
    pub comments: Option<String>,
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

    pub fn success_with_comments(value: T, client_id: u32, server_id: u32, comments: String) -> Self {
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
