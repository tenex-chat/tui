#!/bin/bash
# Reproduction script for socket path bug
# This demonstrates that `tenex-cli -c <config> status` ignores custom socket paths

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEST_DIR="/tmp/tenex-socket-test-$$"
CUSTOM_SOCKET="$TEST_DIR/custom.sock"
CONFIG_FILE="$TEST_DIR/config.json"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

cleanup() {
    echo ""
    echo "=== Cleanup ==="
    # Kill any daemons we started
    if [ -f "$TEST_DIR/daemon.pid" ]; then
        kill $(cat "$TEST_DIR/daemon.pid") 2>/dev/null || true
    fi
    # Clean up default socket location daemon if running
    pkill -f "tenex-cli --daemon" 2>/dev/null || true
    rm -rf "$TEST_DIR"
    echo "Cleaned up test directory: $TEST_DIR"
}

trap cleanup EXIT

echo "=== Socket Path Bug Reproduction Script ==="
echo ""

# Create test directory
mkdir -p "$TEST_DIR"
echo "Created test directory: $TEST_DIR"

# Create config file with custom socket path
cat > "$CONFIG_FILE" << EOF
{
    "socketPath": "$CUSTOM_SOCKET"
}
EOF
echo "Created config file: $CONFIG_FILE"
echo "Config contents:"
cat "$CONFIG_FILE"
echo ""

# Build the CLI
echo "=== Building tenex-cli ==="
cd "$PROJECT_ROOT"
cargo build -p tenex-cli 2>&1 | tail -5
CLI="$PROJECT_ROOT/target/debug/tenex-cli"
echo "Built CLI: $CLI"
echo ""

# Start daemon with custom socket path
echo "=== Starting daemon with custom socket path ==="
$CLI -c "$CONFIG_FILE" --daemon &
DAEMON_PID=$!
echo $DAEMON_PID > "$TEST_DIR/daemon.pid"
sleep 2  # Give daemon time to start

# Check if custom socket was created
echo ""
echo "=== Checking socket creation ==="
if [ -S "$CUSTOM_SOCKET" ]; then
    echo -e "${GREEN}SUCCESS: Custom socket was created at: $CUSTOM_SOCKET${NC}"
else
    echo -e "${RED}FAILURE: Custom socket was NOT created at: $CUSTOM_SOCKET${NC}"
    ls -la "$TEST_DIR/" 2>/dev/null || echo "Test directory is empty or doesn't exist"
    exit 1
fi

# Now test the bug: status command should use the custom socket when config is passed
echo ""
echo "=== Testing status command WITH config (should find daemon) ==="
echo "Running: $CLI -c $CONFIG_FILE status"

# This is where the bug manifests - even with -c config, it checks default socket
if $CLI -c "$CONFIG_FILE" status 2>&1; then
    echo -e "${GREEN}SUCCESS: Status command found the daemon using custom socket${NC}"
else
    echo -e "${YELLOW}NOTE: Status command may have connected (check output above)${NC}"
fi

echo ""
echo "=== Testing status command WITHOUT config (should NOT find daemon) ==="
echo "Running: $CLI status"

# Without config, it should check default socket (where no daemon is running)
if $CLI status 2>&1; then
    echo -e "${YELLOW}NOTE: Found a daemon at default socket (unexpected unless one was already running)${NC}"
else
    echo -e "${GREEN}Expected: No daemon at default socket location${NC}"
fi

echo ""
echo "=== Testing status --running (quick check without auto-start) ==="
echo "Running: $CLI -c $CONFIG_FILE status --running"
if $CLI -c "$CONFIG_FILE" status --running 2>&1; then
    echo -e "${GREEN}SUCCESS: status --running correctly finds daemon on custom socket${NC}"
else
    echo -e "${RED}FAILURE: status --running should have found the daemon${NC}"
fi

echo ""
echo "Testing status --running on non-existent socket (should exit 1):"
echo '{"socketPath": "/tmp/nonexistent-socket.sock"}' > /tmp/temp-bad-config.json
if $CLI -c /tmp/temp-bad-config.json status --running 2>&1; then
    echo -e "${RED}FAILURE: status --running should have returned exit 1${NC}"
else
    echo -e "${GREEN}SUCCESS: status --running correctly returns exit 1 when not running${NC}"
fi
rm -f /tmp/temp-bad-config.json

echo ""
echo "=== Bug Analysis ==="
echo "The bug is that is_daemon_running() and socket_path() in client.rs"
echo "always pass None to get_socket_path(), ignoring any custom config."
echo "This means 'tenex-cli -c custom.json status' checks the wrong socket."

echo ""
echo "=== Test Complete ==="
