#!/bin/bash
# Installation script for cooling-control

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

echo -e "${GREEN}Installing cooling-control...${NC}"

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo -e "${RED}Please run as root (use sudo)${NC}"
    exit 1
fi

# Create directories
echo "Creating directories..."
mkdir -p /opt/cooling-control
mkdir -p /etc/cooling-control

# Build Rust binary
echo "Building Rust binary..."
cargo build --release
cp target/release/cooling-control /opt/cooling-control/
chmod +x /opt/cooling-control/cooling-control

# Install systemd service
echo "Installing systemd service..."
cp cooling-control.service /etc/systemd/system/
systemctl daemon-reload

# Migrate config from old location if present
if [ -f /etc/liquidctl-monitor/config.json ] && [ ! -f /etc/cooling-control/config.json ]; then
    echo "Migrating config from /etc/liquidctl-monitor/config.json..."
    cp /etc/liquidctl-monitor/config.json /etc/cooling-control/config.json
fi

# Create default config if it doesn't exist
if [ ! -f /etc/cooling-control/config.json ]; then
    echo "Creating default configuration..."
    cat > /etc/cooling-control/config.json << 'EOF'
{
    "monitoring": {
        "interval": 2.0,
        "history_size": 10,
        "smoothing_factor": 0.2
    },
    "fan_curve": {
        "radiator_profile": [20, 20, 30, 40, 35, 60, 40, 80, 45, 100],
        "motherboard_profile": [30, 30, 40, 50, 50, 70, 60, 85, 70, 100]
    },
    "pump_curve": {
        "profile": [30, 30, 40, 50, 50, 70, 60, 85, 70, 100]
    },
    "hardware": {
        "quadro_device": "auto",
        "d5_device": "auto"
    },
    "temperature_limits": {
        "cpu_max": 95.0,
        "gpu_max": 90.0,
        "coolant_max": 50.0,
        "motherboard_max": 80.0
    }
}
EOF
fi

# Set permissions
chown -R root:root /opt/cooling-control
chown -R root:root /etc/cooling-control

# Enable and start service
echo "Enabling and starting service..."
systemctl enable cooling-control.service
systemctl start cooling-control.service

# Check status
echo "Checking service status..."
if systemctl is-active --quiet cooling-control.service; then
    echo -e "${GREEN}Service started successfully!${NC}"
else
    echo -e "${RED}Service failed to start. Check logs with: journalctl -u cooling-control${NC}"
    exit 1
fi

echo -e "${GREEN}Installation complete!${NC}"
echo ""
echo "Useful commands:"
echo "  Check status: systemctl status cooling-control"
echo "  View logs:    journalctl -u cooling-control -f"
echo "  Restart:      systemctl restart cooling-control"
echo ""
echo "Configuration: /etc/cooling-control/config.json"
