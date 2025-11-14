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
}

impl<T: Default> AlpacaResponse<T> {
    pub fn success(value: T, client_id: u32, server_id: u32) -> Self {
        Self {
            value,
            client_transaction_i_d: client_id,
            server_transaction_i_d: server_id,
            error_number: 0,
            error_message: String::new(),
        }
    }

    pub fn error(client_id: u32, server_id: u32, error_number: i32, error_message: String) -> Self {
        Self {
            value: T::default(),
            client_transaction_i_d: client_id,
            server_transaction_i_d: server_id,
            error_number,
            error_message,
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
