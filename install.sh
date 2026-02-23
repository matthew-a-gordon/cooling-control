#!/bin/bash
# Installation script for liquidctl temperature monitor

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}Installing Liquidctl Temperature Monitor...${NC}"

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo -e "${RED}Please run as root (use sudo)${NC}"
    exit 1
fi

# Create directories
echo "Creating directories..."
mkdir -p /opt/liquidctl-monitor
mkdir -p /etc/liquidctl-monitor
mkdir -p /var/log/liquidctl-monitor

# Build Rust binary
echo "Building Rust binary..."
cargo build --release
cp target/release/liquidctl-monitor /opt/liquidctl-monitor/
chmod +x /opt/liquidctl-monitor/liquidctl-monitor

# Install systemd service
echo "Installing systemd service..."
cp liquidctl-monitor.service /etc/systemd/system/
systemctl daemon-reload

# Create default config if it doesn't exist
if [ ! -f /etc/liquidctl-monitor/config.json ]; then
    echo "Creating default configuration..."
    cat > /etc/liquidctl-monitor/config.json << 'EOF'
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
chown -R root:root /opt/liquidctl-monitor
chown -R root:root /etc/liquidctl-monitor
chown -R root:root /var/log/liquidctl-monitor

# Enable and start service
echo "Enabling and starting service..."
systemctl enable liquidctl-monitor.service
systemctl start liquidctl-monitor.service

# Check status
echo "Checking service status..."
if systemctl is-active --quiet liquidctl-monitor.service; then
    echo -e "${GREEN}Service started successfully!${NC}"
else
    echo -e "${RED}Service failed to start. Check logs with: journalctl -u liquidctl-monitor.service${NC}"
    exit 1
fi

echo -e "${GREEN}Installation complete!${NC}"
echo ""
echo "Useful commands:"
echo "  Check status: systemctl status liquidctl-monitor.service"
echo "  View logs: journalctl -u liquidctl-monitor.service -f"
echo "  Stop service: systemctl stop liquidctl-monitor.service"
echo "  Start service: systemctl start liquidctl-monitor.service"
echo "  Restart service: systemctl restart liquidctl-monitor.service"
echo ""
echo "Configuration file: /etc/liquidctl-monitor/config.json"
echo "Log file: /var/log/liquidctl-monitor/monitor.log"
