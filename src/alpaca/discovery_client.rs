use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket, IpAddr};
use std::time::Duration;
use tracing::{debug, info, warn, error};

/// Discovered Alpaca device information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredDevice {
    pub host: String,
    pub port: u16,
    pub devices: Vec<DeviceInfo>,
}

/// Information about a single device on an Alpaca server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_name: String,
    pub device_type: String,
    pub device_number: u32,
    pub unique_id: String,
}

/// Response from Alpaca discovery broadcast
#[derive(Debug, Deserialize)]
struct DiscoveryResponse {
    #[serde(rename = "AlpacaPort")]
    alpaca_port: u16,
}

/// Response from management API configureddevices
#[derive(Debug, Deserialize)]
struct ConfiguredDevicesResponse {
    #[serde(rename = "Value")]
    value: Vec<ConfiguredDevice>,
}

#[derive(Debug, Deserialize)]
struct ConfiguredDevice {
    #[serde(rename = "DeviceName")]
    device_name: String,
    #[serde(rename = "DeviceType")]
    device_type: String,
    #[serde(rename = "DeviceNumber")]
    device_number: u32,
    #[serde(rename = "UniqueID")]
    unique_id: String,
}

/// Alpaca Discovery Client - finds Alpaca devices on the network
pub struct DiscoveryClient {
    timeout: Duration,
    http_client: Client,
}

impl DiscoveryClient {
    pub fn new(timeout_seconds: u64) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .build()
            .unwrap();

        Self {
            timeout: Duration::from_secs(timeout_seconds),
            http_client,
        }
    }

    /// Discover all Alpaca devices on the local network
    /// Uses UDP broadcast on port 32227 to find devices
    pub async fn discover(&self) -> Vec<DiscoveredDevice> {
        let mut discovered = Vec::new();
        let mut seen_hosts: HashMap<String, u16> = HashMap::new();

        // Try to discover via UDP broadcast
        match self.discover_via_broadcast().await {
            Ok(hosts) => {
                for (host, port) in hosts {
                    let key = format!("{}:{}", host, port);
                    if seen_hosts.contains_key(&key) {
                        continue;
                    }
                    seen_hosts.insert(key, port);

                    // Query the management API to get device list
                    match self.get_device_list(&host, port).await {
                        Ok(devices) => {
                            discovered.push(DiscoveredDevice {
                                host: host.clone(),
                                port,
                                devices,
                            });
                        }
                        Err(e) => {
                            warn!("Failed to get device list from {}:{}: {}", host, port, e);
                            // Still add the host, just with empty device list
                            discovered.push(DiscoveredDevice {
                                host: host.clone(),
                                port,
                                devices: Vec::new(),
                            });
                        }
                    }
                }
            }
            Err(e) => {
                warn!("Discovery broadcast failed: {}", e);
            }
        }

        discovered
    }

    /// Send discovery broadcast and collect responses
    async fn discover_via_broadcast(&self) -> Result<Vec<(String, u16)>, Box<dyn std::error::Error + Send + Sync>> {
        let timeout = self.timeout;

        // Run the UDP discovery in a blocking task since UdpSocket is synchronous
        let result = tokio::task::spawn_blocking(move || {
            Self::discover_blocking(timeout)
        }).await??;

        Ok(result)
    }

    fn discover_blocking(timeout: Duration) -> Result<Vec<(String, u16)>, Box<dyn std::error::Error + Send + Sync>> {
        let mut discovered = Vec::new();

        // Create UDP socket
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.set_broadcast(true)?;
        socket.set_read_timeout(Some(timeout))?;

        // Send discovery broadcast to 255.255.255.255:32227
        let discovery_msg = b"alpacadiscovery1";
        let broadcast_addr: SocketAddr = "255.255.255.255:32227".parse()?;

        match socket.send_to(discovery_msg, broadcast_addr) {
            Ok(_) => {
                info!("Sent Alpaca discovery broadcast");
            }
            Err(e) => {
                warn!("Failed to send discovery broadcast: {}", e);
                return Err(Box::new(e));
            }
        }

        // Collect responses
        let mut buf = [0u8; 1024];
        let start = std::time::Instant::now();

        while start.elapsed() < timeout {
            match socket.recv_from(&mut buf) {
                Ok((len, src)) => {
                    let response_data = &buf[..len];
                    if let Ok(response_str) = std::str::from_utf8(response_data) {
                        if let Ok(response) = serde_json::from_str::<DiscoveryResponse>(response_str) {
                            let host = src.ip().to_string();
                            info!("Discovered Alpaca device at {}:{}", host, response.alpaca_port);
                            discovered.push((host, response.alpaca_port));
                        }
                    }
                }
                Err(e) => {
                    // Timeout or other error - stop collecting
                    if e.kind() != std::io::ErrorKind::WouldBlock && e.kind() != std::io::ErrorKind::TimedOut {
                        debug!("Discovery receive error: {}", e);
                    }
                    break;
                }
            }
        }

        Ok(discovered)
    }

    /// Query the Alpaca management API to get list of configured devices
    async fn get_device_list(&self, host: &str, port: u16) -> Result<Vec<DeviceInfo>, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("http://{}:{}/management/v1/configureddevices", host, port);

        let response = self.http_client
            .get(&url)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()).into());
        }

        let configured: ConfiguredDevicesResponse = response.json().await?;

        Ok(configured.value
            .into_iter()
            .map(|d| DeviceInfo {
                device_name: d.device_name,
                device_type: d.device_type,
                device_number: d.device_number,
                unique_id: d.unique_id,
            })
            .collect())
    }

    /// Check if a specific host has an Alpaca server
    pub async fn probe_host(&self, host: &str, port: u16) -> Option<DiscoveredDevice> {
        match self.get_device_list(host, port).await {
            Ok(devices) => {
                Some(DiscoveredDevice {
                    host: host.to_string(),
                    port,
                    devices,
                })
            }
            Err(e) => {
                debug!("Probe failed for {}:{}: {}", host, port, e);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovery_client_creation() {
        let client = DiscoveryClient::new(5);
        assert_eq!(client.timeout, Duration::from_secs(5));
    }
}
