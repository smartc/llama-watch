#!/bin/bash
# Installation script for llama-watch systemd service
# Run with sudo: sudo ./install-service.sh

set -e

# Configuration
SERVICE_NAME="llama-watch"
SERVICE_USER="llama-watch"
INSTALL_DIR="/usr/local/bin"
DATA_DIR="/var/lib/llama-watch"
STATIC_DIR="/usr/share/llama-watch"
SOURCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "Installing LLAMA Safety Monitor service..."

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo "Error: Please run as root (sudo ./install-service.sh)"
    exit 1
fi

# Check if binary exists
if [ ! -f "$SOURCE_DIR/target/release/llama-watch" ]; then
    echo "Error: Binary not found. Please build first with: cargo build --release"
    exit 1
fi

# Create service user if it doesn't exist
if ! id "$SERVICE_USER" &>/dev/null; then
    echo "Creating service user: $SERVICE_USER"
    useradd --system --no-create-home --shell /usr/sbin/nologin "$SERVICE_USER"
fi

# Create directories
echo "Creating directories..."
mkdir -p "$DATA_DIR"
mkdir -p "$STATIC_DIR"

# Copy binary
echo "Installing binary to $INSTALL_DIR..."
cp "$SOURCE_DIR/target/release/llama-watch" "$INSTALL_DIR/llama-watch"
chmod 755 "$INSTALL_DIR/llama-watch"

# Copy static files
echo "Installing static files to $STATIC_DIR..."
cp -r "$SOURCE_DIR/static" "$STATIC_DIR/"

# Create symlink for static files in data directory
ln -sf "$STATIC_DIR/static" "$DATA_DIR/static"

# Copy existing config if present
if [ -f "$SOURCE_DIR/config.json" ]; then
    echo "Copying existing configuration..."
    cp "$SOURCE_DIR/config.json" "$DATA_DIR/config.json"
fi

# Set ownership
echo "Setting permissions..."
chown -R "$SERVICE_USER:$SERVICE_USER" "$DATA_DIR"
chown -R root:root "$STATIC_DIR"

# Install service file
echo "Installing systemd service..."
cp "$SOURCE_DIR/llama-watch.service" /etc/systemd/system/llama-watch.service
chmod 644 /etc/systemd/system/llama-watch.service

# Reload systemd
systemctl daemon-reload

echo ""
echo "Installation complete!"
echo ""
echo "To enable and start the service:"
echo "  sudo systemctl enable llama-watch"
echo "  sudo systemctl start llama-watch"
echo ""
echo "To check status:"
echo "  sudo systemctl status llama-watch"
echo ""
echo "To view logs:"
echo "  sudo journalctl -u llama-watch -f"
echo ""
echo "Configuration file location: $DATA_DIR/config.json"
echo "Web UI will be available at: http://localhost:8080"
