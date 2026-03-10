#!/bin/bash
# Build script for Fawx WASM skills

set -e

echo "Building Fawx skills..."

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

# Build vision skill
echo "Building vision-skill..."
cd vision-skill
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/vision_skill.wasm vision.wasm
echo "✓ vision-skill built -> vision.wasm"
cd ..

# Build TTS skill
echo "Building tts-skill..."
cd tts-skill
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/tts_skill.wasm tts.wasm
echo "✓ tts-skill built -> tts.wasm"
cd ..

# Build browser skill
echo "Building browser-skill..."
cd browser-skill
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/browser_skill.wasm browser.wasm
echo "✓ browser-skill built -> browser.wasm"
cd ..

echo "Building canvas-skill..."
cd canvas-skill
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/canvas_skill.wasm canvas.wasm
echo "✓ canvas-skill built -> canvas.wasm"
cd ..

echo ""
echo "All skills built successfully!"
echo ""
echo "To install skills:"
echo "  fawx skill install skills/weather-skill/weather.wasm"
echo "  fawx skill install skills/calculator-skill/calculator.wasm"
echo "  fawx skill install skills/vision-skill/vision.wasm"
echo "  fawx skill install skills/tts-skill/tts.wasm"
echo "  fawx skill install skills/browser-skill/browser.wasm"
echo "  fawx skill install skills/canvas-skill/canvas.wasm"
