# Justfile for Nova - Common development commands

# Default recipe (run with just `just`)
default:
    @just --list

# Build all crates in the workspace
build:
    cargo build --workspace

# Build release version
build-release:
    cargo build --workspace --release

# Run all tests
test:
    cargo test --workspace

# Run tests with output
test-verbose:
    cargo test --workspace -- --nocapture

# Run clippy linter
lint:
    cargo clippy --workspace -- -D warnings

# Format all code
fmt:
    cargo fmt --all

# Check formatting without modifying files
fmt-check:
    cargo fmt --all --check

# Run all checks (format, lint, test)
check: fmt-check lint test
    @echo "✓ All checks passed!"

# Clean build artifacts
clean:
    cargo clean

# Run the CLI
run *ARGS:
    cargo run -p nv-cli -- {{ARGS}}

# Run doctor command
doctor:
    cargo run -p nv-cli -- doctor

# Watch and rebuild on file changes (requires cargo-watch)
watch:
    cargo watch -x "check --workspace"

# Check Android cross-compilation (no binary output)
check-android:
    cargo check --target aarch64-linux-android

# Build for Android (Horizon 1)
build-android:
    cargo build --target aarch64-linux-android --release
