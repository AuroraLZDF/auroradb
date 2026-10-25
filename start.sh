#!/bin/bash

# MySQL Client Development Startup Script

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "Starting MySQL Client..."

# Start Tauri
cd "$SCRIPT_DIR/src-tauri"
cargo tauri dev --no-dev-server
