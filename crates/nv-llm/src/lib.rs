//! LLM provider abstraction (local + cloud).
//!
//! Provides unified interface for both local LLM inference (llama.cpp)
//! and cloud LLM APIs (Claude), with automatic routing based on complexity.

use nv_core::error::LlmError;

/// LLM provider trait.
///
/// Abstraction over local and cloud LLM providers.
pub trait LlmProvider {
    /// Generate a completion for the given prompt.
    fn complete(&self, prompt: &str) -> Result<String, LlmError>;
}

/// Local LLM provider (llama.cpp).
#[derive(Default)]
pub struct LocalModel {
    // Placeholder - will be implemented in Epic 2
}

/// Cloud LLM provider (Claude API).
#[derive(Default)]
pub struct CloudModel {
    // Placeholder - will be implemented in Epic 3
}
