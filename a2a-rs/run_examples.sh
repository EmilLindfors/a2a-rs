#!/bin/bash

# A2A Examples Runner Script
# This script runs the HTTP and WebSocket server/client examples against each other
# with proper cleanup and exit handling.

set -e  # Exit on error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Function to kill background processes
cleanup() {
    print_info "Cleaning up background processes..."
    if [[ -n "$HTTP_SERVER_PID" ]] && kill -0 "$HTTP_SERVER_PID" 2>/dev/null; then
        print_info "Stopping HTTP server (PID: $HTTP_SERVER_PID)"
        kill "$HTTP_SERVER_PID" 2>/dev/null || true
        wait "$HTTP_SERVER_PID" 2>/dev/null || true
    fi
    
    if [[ -n "$WS_SERVER_PID" ]] && kill -0 "$WS_SERVER_PID" 2>/dev/null; then
        print_info "Stopping WebSocket server (PID: $WS_SERVER_PID)"
        kill "$WS_SERVER_PID" 2>/dev/null || true
        wait "$WS_SERVER_PID" 2>/dev/null || true
    fi
    
    # Additional cleanup - kill any remaining cargo processes
    print_info "Ensuring all cargo example processes are stopped..."
    pkill -f "cargo.*example.*server" 2>/dev/null || true
    sleep 1
    
    print_success "Cleanup completed"
}

# Set up signal handlers for cleanup
trap cleanup EXIT INT TERM

# Function to wait for a port to be available
wait_for_port() {
    local port=$1
    local max_attempts=${2:-30}
    local attempt=0
    
    print_info "Waiting for port $port to be available..."
    while [[ $attempt -lt $max_attempts ]]; do
        # Try to connect to the port using /dev/tcp
        if timeout 1 bash -c "</dev/tcp/localhost/$port" 2>/dev/null; then
            print_success "Port $port is available"
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 1
    done
    
    print_error "Port $port did not become available after $max_attempts seconds"
    return 1
}

# Function to check if all required tools are available
check_dependencies() {
    local missing_deps=()
    
    if ! command -v cargo >/dev/null 2>&1; then
        missing_deps+=("cargo")
    fi
    
    if ! command -v timeout >/dev/null 2>&1; then
        missing_deps+=("timeout")
    fi
    
    if [[ ${#missing_deps[@]} -gt 0 ]]; then
        print_error "Missing required dependencies: ${missing_deps[*]}"
        print_info "Please install the missing dependencies and try again"
        exit 1
    fi
    
    print_success "All dependencies are available"
}

# Function to build the project
build_project() {
    print_info "Building the project..."
    if cargo build --examples --features="full"; then
        print_success "Project built successfully"
    else
        print_error "Failed to build project"
        exit 1
    fi
}

# Function to run builder patterns example
run_builder_patterns() {
    print_info "Running builder patterns example..."
    echo "========================================="
    if cargo run --example builder_patterns; then
        print_success "Builder patterns example completed"
    else
        print_error "Builder patterns example failed"
        return 1
    fi
    echo "========================================="
    echo
}

# Function to run HTTP examples
run_http_examples() {
    print_info "Running HTTP server/client examples..."
    echo "========================================="
    
    # Start HTTP server in background
    print_info "Starting HTTP server..."
    cargo run --example http_server --features="http-server" &
    HTTP_SERVER_PID=$!
    
    # Wait for server to be ready
    if wait_for_port 8080; then
        print_success "HTTP server is running (PID: $HTTP_SERVER_PID)"
        
        # Run HTTP client
        print_info "Running HTTP client..."
        if cargo run --example http_client --features="http-client"; then
            print_success "HTTP client completed successfully"
        else
            print_error "HTTP client failed"
            return 1
        fi
    else
        print_error "HTTP server failed to start"
        return 1
    fi
    
    echo "========================================="
    echo
}

# Function to run WebSocket examples
run_websocket_examples() {
    print_info "Running WebSocket server/client examples..."
    echo "========================================="
    
    # Start WebSocket server in background
    print_info "Starting WebSocket server..."
    cargo run --example websocket_server --features="ws-server" &
    WS_SERVER_PID=$!
    
    # Wait for server to be ready
    if wait_for_port 8081; then
        print_success "WebSocket server is running (PID: $WS_SERVER_PID)"
        
        # Run WebSocket client
        print_info "Running WebSocket client..."
        if cargo run --example websocket_client --features="ws-client"; then
            print_success "WebSocket client completed successfully"
        else
            print_error "WebSocket client failed"
            return 1
        fi
    else
        print_error "WebSocket server failed to start"
        return 1
    fi
    
    echo "========================================="
    echo
}

# Main execution
main() {
    print_info "Starting A2A Examples Runner"
    echo
    
    # Check dependencies
    check_dependencies
    echo
    
    # Build project
    build_project
    echo
    
    # Track overall success
    local overall_success=true
    
    # Run builder patterns example (standalone)
    if ! run_builder_patterns; then
        overall_success=false
    fi
    
    # Run HTTP examples
    if ! run_http_examples; then
        overall_success=false
    fi
    
    # Clean up before running WebSocket examples
    cleanup
    sleep 2  # Give ports time to be released
    
    # Run WebSocket examples
    if ! run_websocket_examples; then
        overall_success=false
    fi
    
    # Summary
    echo
    print_info "========================================="
    if $overall_success; then
        print_success "All examples completed successfully!"
        print_info "The A2A protocol implementation is working correctly."
    else
        print_error "Some examples failed."
        print_info "Check the output above for details."
        exit 1
    fi
    print_info "========================================="
}

# Run main function
main "$@"