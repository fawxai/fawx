# Vendoring llama.cpp

This document describes how to vendor llama.cpp source code for compilation.

## Pinned Version

**Target release:** `b1696` (llama.cpp stable release, Feb 2024)  
**Repository:** https://github.com/ggerganov/llama.cpp  
**Commit SHA:** _To be filled when vendoring_ (tag `b1696` resolves to specific commit)

> **Why b1696?** This is a stable release with proven Android compatibility and GGUF format support.

## Vendoring Instructions

### Option 1: Git Submodule (Recommended)

```bash
# From fawx repo root
cd engine/crates/llama-cpp-sys
git submodule add -b b1696 https://github.com/ggerganov/llama.cpp vendor/llama.cpp
git submodule update --init --recursive
```

### Option 2: Manual Download

```bash
cd engine/crates/llama-cpp-sys
mkdir -p vendor
cd vendor
wget https://github.com/ggerganov/llama.cpp/archive/refs/tags/b1696.tar.gz
tar xzf b1696.tar.gz
mv llama.cpp-b1696 llama.cpp
```

## Build Script Integration

Once vendored, update `build.rs` to compile the C++ source:

```rust
#[cfg(feature = "llama-cpp")]
{
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .file("vendor/llama.cpp/llama.cpp")
        .file("vendor/llama.cpp/ggml.c")
        .file("vendor/llama.cpp/ggml-alloc.c")
        .file("vendor/llama.cpp/ggml-backend.c")
        .flag_if_supported("-std=c++11")
        .flag_if_supported("-O3")
        .flag_if_supported("-march=native")
        .compile("llama");

    println!("cargo:rustc-link-lib=static=llama");
}
```

## Android-Specific Configuration

For `aarch64-linux-android` target, add:

```rust
if env::var("CARGO_CFG_TARGET_OS").unwrap() == "android" {
    build.flag("-DGGML_USE_CPU");  // CPU-only for now
    // Future: Add GPU acceleration
    // build.flag("-DGGML_USE_VULKAN");
}
```

## Verification

After vendoring, test compilation:

```bash
# With feature flag
cargo build --features llama-cpp

# For Android
cargo build --target aarch64-linux-android --features llama-cpp
```

## License Compatibility

- **llama.cpp:** MIT License
- **Nova (this project):** MIT License
- ✅ **Compatible:** Safe to vendor and redistribute

## Updating llama.cpp

To update to a newer commit:

1. Update the pinned commit in this file
2. Update the submodule: `git submodule update --remote vendor/llama.cpp`
3. Test compilation and runtime behavior
4. Document breaking changes in CHANGELOG.md

## Future: Alternative Backends

- **llama-cpp-python:** Python bindings (for desktop testing)
- **candle:** Pure Rust alternative (simpler but less optimized)
- **ggml-rs:** Direct GGML bindings (more control, more complexity)

For now, llama.cpp C++ is the proven choice for Android production use.
