#!/bin/bash
# Build script for Nova WASM skills

set -e

echo "Building Nova skills..."

# Ensure wasm32-unknown-unknown target is installed
rustup target add wasm32-unknown-unknown

# Build weather skill
echo "Building weather-skill..."
cd weather-skill
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/weather_skill.wasm weather.wasm
echo "✓ weather-skill built -> weather.wasm"
cd ..

# Build calculator skill
echo "Building calculator-skill..."
cd calculator-skill
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/calculator_skill.wasm calculator.wasm
echo "✓ calculator-skill built -> calculator.wasm"
cd ..

echo ""
echo "All skills built successfully!"
echo ""
echo "To install skills:"
echo "  nova skill install skills/weather-skill/weather.wasm"
echo "  nova skill install skills/calculator-skill/calculator.wasm"
