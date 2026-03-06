//! WASM skill runtime with capability enforcement.
//!
//! Loads and executes signed WASM skills with enforced security boundaries.
//!
//! # Architecture
//!
//! - **Manifest**: Skill metadata, capabilities, and API version contract
//! - **HostApi**: Host functions exposed to WASM skills (host_api_v1)
//! - **Capabilities**: Runtime enforcement of declared permissions
//! - **Cache**: Module compilation caching for faster loading
//! - **Storage**: Isolated key-value storage with quota enforcement
//! - **Signing**: Ed25519 signature generation and verification
//! - **Loader**: Skill loading with signature verification
//! - **Runtime**: Skill registration and invocation

pub mod cache;
pub mod capabilities;
pub mod host_api;
pub mod live_host_api;
pub mod loader;
pub mod manifest;
pub mod registry;
pub mod runtime;
pub mod signing;
pub mod storage;

// Re-export commonly used types
pub use cache::{cache_stats, clear_cache, compile_module, has_cached_module, CacheStats};
pub use capabilities::CapabilityChecker;
pub use host_api::{HostApi, HostApiBase, MockHostApi};
pub use live_host_api::{LiveHostApi, LiveHostApiConfig};
pub use loader::{LoadedSkill, SkillLoader};
pub use manifest::{parse_manifest, validate_manifest, Capability, SkillManifest};
pub use registry::SkillRegistry;
pub use runtime::SkillRuntime;
pub use signing::{generate_keypair, sign_skill, verify_skill};
pub use storage::SkillStorage;
