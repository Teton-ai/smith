#!/bin/bash
# Build Docker images required for integration tests

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"

echo "Building Docker images for integration tests..."

# Build API image for tests
echo "Building API test image..."
docker build -f api.Dockerfile -t smith-api:test .

# Build device image for tests (use ubuntu for faster builds)
echo "Building device test image..."
docker build -f device.Dockerfile \
    --build-arg BASE_IMAGE=ubuntu:22.04 \
    -t smith-device:test .

echo "✓ Test images built successfully!"
echo ""
echo "You can now run integration tests with:"
echo "  cargo test --package smith --test '*' -- --ignored --nocapture"
