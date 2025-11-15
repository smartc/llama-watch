use std::net::{UdpSocket, SocketAddr};
use std::sync::Arc;
use tokio::task;
use tracing::{info, error, warn};

/// ASCOM Alpaca Discovery Service
/// Listens on UDP port 32227 for discovery broadcasts and responds with server information
pub struct DiscoveryService {
    alpaca_port: u16,
    device_name: String,
}

impl DiscoveryService {
    pub fn new(alpaca_port: u16, device_name: String) -> Self {
        Self {
            alpaca_port,
            device_name,
        }
    }

    /// Start the discovery service in a background task
    pub fn start(self: Arc<Self>) {
        task::spawn_blocking(move || {
            if let Err(e) = self.run() {
                error!("Discovery service error: {}", e);
            }
        });
    }

    /// Run the discovery service (blocking)
    fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Bind to all interfaces on port 32227
        let socket = UdpSocket::bind("0.0.0.0:32227")?;
        socket.set_broadcast(true)?;

        info!("ASCOM Alpaca discovery service listening on UDP port 32227");

        let mut buf = [0u8; 1024];

        loop {
            match socket.recv_from(&mut buf) {
                Ok((len, src)) => {
                    let data = &buf[..len];

                    // ASCOM Alpaca discovery packet is "alpacadiscovery1"
                    if data == b"alpacadiscovery1" {
                        info!("Received Alpaca discovery request from {}", src);
                        self.send_response(&socket, src);
                    } else {
                        warn!("Received unknown discovery packet from {}: {:?}", src,
                              String::from_utf8_lossy(data));
                    }
                }
                Err(e) => {
                    error!("Error receiving discovery packet: {}", e);
                }
            }
        }
    }

    /// Send discovery response
    fn send_response(&self, socket: &UdpSocket, dest: SocketAddr) {
        // Response format: {"AlpacaPort": port_number}
        let response = serde_json::json!({
            "AlpacaPort": self.alpaca_port
        });

        let response_bytes = response.to_string().into_bytes();

        match socket.send_to(&response_bytes, dest) {
            Ok(_) => {
                info!("Sent discovery response to {}: AlpacaPort={}", dest, self.alpaca_port);
            }
            Err(e) => {
                error!("Failed to send discovery response to {}: {}", dest, e);
            }
        }
    }
}
