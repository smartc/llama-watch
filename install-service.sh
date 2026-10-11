#!/bin/bash
# Installation script for llama-watch systemd service
# Run with sudo: sudo ./install-service.sh
# Safe to run again to upgrade: the existing configuration is kept.

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

# Install binary. `install` replaces the destination itself, so if
# $INSTALL_DIR/llama-watch is a symlink the file it points to is left alone.
echo "Installing binary to $INSTALL_DIR..."
install -m 755 "$SOURCE_DIR/target/release/llama-watch" "$INSTALL_DIR/llama-watch"

# Install static files, replacing any previous copy so files removed in a
# newer version don't linger
echo "Installing static files to $STATIC_DIR..."
rm -rf "$STATIC_DIR/static.new"
cp -r "$SOURCE_DIR/static" "$STATIC_DIR/static.new"
rm -rf "$STATIC_DIR/static"
mv "$STATIC_DIR/static.new" "$STATIC_DIR/static"

# Link static files into the data directory. -n replaces an existing link
# instead of following it and creating a link inside the static directory.
ln -sfn "$STATIC_DIR/static" "$DATA_DIR/static"

# Copy a configuration only on first install; never overwrite the live one,
# which holds changes made in the web UI
if [ -f "$DATA_DIR/config.json" ]; then
    echo "Keeping existing configuration in $DATA_DIR/config.json"
elif [ -f "$SOURCE_DIR/config.json" ]; then
    echo "Copying configuration..."
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
if systemctl is-active --quiet "$SERVICE_NAME"; then
    echo "The service is running the previous version. Restart it to use this one:"
    echo "  sudo systemctl restart $SERVICE_NAME"
else
    echo "To enable and start the service:"
    echo "  sudo systemctl enable $SERVICE_NAME"
    echo "  sudo systemctl start $SERVICE_NAME"
fi
echo ""
echo "To check status:"
echo "  sudo systemctl status $SERVICE_NAME"
echo ""
