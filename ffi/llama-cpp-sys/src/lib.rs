//! Raw FFI bindings to llama.cpp
//!
//! This crate provides low-level C bindings to llama.cpp for on-device LLM inference.
//! For safe Rust wrappers, use the `nv-llm` crate.
//!
//! ## Feature Flags
//!
//! - `llama-cpp`: Enable actual llama.cpp FFI (requires vendored source)
//!
//! ## Safety
//!
//! All functions in this crate are `unsafe` as they directly call C code.
//! Users must ensure:
//! - Pointers are valid and properly aligned
//! - Memory is not freed while still in use
//! - Thread safety is maintained (llama.cpp is not thread-safe by default)

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_float, c_int};

// Opaque types (llama.cpp internal structures)

/// Opaque handle to a llama.cpp model.
///
/// # Safety
/// This type must only be created and destroyed by llama.cpp via
/// `llama_model_load` and `llama_free_model`. Rust code should only
/// pass pointers to this type, never dereference or construct directly.
/// The `[u8; 0]` pattern ensures zero size while preventing construction.
#[repr(C)]
pub struct llama_model {
    _private: [u8; 0],
}

/// Opaque handle to a llama.cpp inference context.
///
/// # Safety
/// This type must only be created and destroyed by llama.cpp via
/// `llama_new_context` and `llama_free`. Rust code should only pass
/// pointers to this type, never dereference or construct directly.
/// The context is NOT thread-safe; synchronize access externally.
#[repr(C)]
pub struct llama_context {
    _private: [u8; 0],
}

pub type llama_token = i32;

// Model parameters
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct llama_model_params {
    pub n_gpu_layers: c_int,
    pub main_gpu: c_int,
    pub use_mmap: bool,
    pub use_mlock: bool,
}

// Context parameters
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct llama_context_params {
    pub n_ctx: c_int,
    pub n_batch: c_int,
    pub n_threads: c_int,
    pub n_threads_batch: c_int,
    pub rope_freq_base: c_float,
    pub rope_freq_scale: c_float,
}

#[cfg(feature = "llama-cpp")]
extern "C" {
    /// Load a model from file
    ///
    /// # Safety
    /// - `path` must be a valid null-terminated C string
    /// - `params` must be a valid pointer to llama_model_params
    /// - Returned pointer must be freed with `llama_free_model`
    pub fn llama_model_load(path: *const c_char, params: llama_model_params) -> *mut llama_model;

    /// Create a new context from a model
    ///
    /// # Safety
    /// - `model` must be a valid model pointer
    /// - `params` must be a valid pointer to llama_context_params
    /// - Returned pointer must be freed with `llama_free`
    pub fn llama_new_context(
        model: *const llama_model,
        params: llama_context_params,
    ) -> *mut llama_context;

    /// Tokenize text into tokens
    ///
    /// # Safety
    /// - `ctx` must be a valid context pointer
    /// - `text` must be a valid null-terminated C string
    /// - `tokens` must be a valid buffer with capacity >= `n_max_tokens`
    /// - Returns number of tokens written, or negative on error
    pub fn llama_tokenize(
        ctx: *const llama_context,
        text: *const c_char,
        tokens: *mut llama_token,
        n_max_tokens: c_int,
        add_bos: bool,
    ) -> c_int;

    /// Decode tokens and update context state
    ///
    /// # Safety
    /// - `ctx` must be a valid context pointer
    /// - `tokens` must be a valid array of `n_tokens` tokens
    /// - Returns 0 on success, non-zero on error
    pub fn llama_decode(
        ctx: *mut llama_context,
        tokens: *const llama_token,
        n_tokens: c_int,
        n_past: c_int,
    ) -> c_int;

    /// Convert a token to a string
    ///
    /// # Safety
    /// - `ctx` must be a valid context pointer
    /// - Returned pointer is owned by llama.cpp, do not free
    /// - Pointer is valid until next llama.cpp API call
    pub fn llama_token_to_str(ctx: *const llama_context, token: llama_token) -> *const c_char;

    /// Free a context
    ///
    /// # Safety
    /// - `ctx` must be a valid context pointer
    /// - Must not be used after calling this function
    pub fn llama_free(ctx: *mut llama_context);

    /// Free a model
    ///
    /// # Safety
    /// - `model` must be a valid model pointer
    /// - Must not be used after calling this function
    /// - All contexts using this model must be freed first
    pub fn llama_free_model(model: *mut llama_model);
}

#[cfg(not(feature = "llama-cpp"))]
pub mod stub {
    //! Stub implementations when llama-cpp feature is disabled
    //!
    //! These exist to allow compilation without vendored llama.cpp source.
    use super::*;

    /// Stub for llama_model_load (always returns null)
    ///
    /// # Safety
    /// This is a no-op stub that always returns null. Safe to call but not functional.
    pub unsafe fn llama_model_load(
        _path: *const c_char,
        _params: llama_model_params,
    ) -> *mut llama_model {
        std::ptr::null_mut()
    }

    /// Stub for llama_new_context (always returns null)
    ///
    /// # Safety
    /// This is a no-op stub that always returns null. Safe to call but not functional.
    pub unsafe fn llama_new_context(
        _model: *const llama_model,
        _params: llama_context_params,
    ) -> *mut llama_context {
        std::ptr::null_mut()
    }

    /// Stub for llama_tokenize (always returns -1)
    ///
    /// # Safety
    /// This is a no-op stub that always returns -1. Safe to call but not functional.
    pub unsafe fn llama_tokenize(
        _ctx: *const llama_context,
        _text: *const c_char,
        _tokens: *mut llama_token,
        _n_max_tokens: c_int,
        _add_bos: bool,
    ) -> c_int {
        -1
    }

    /// Stub for llama_decode (always returns -1)
    ///
    /// # Safety
    /// This is a no-op stub that always returns -1. Safe to call but not functional.
    pub unsafe fn llama_decode(
        _ctx: *mut llama_context,
        _tokens: *const llama_token,
        _n_tokens: c_int,
        _n_past: c_int,
    ) -> c_int {
        -1
    }

    /// Stub for llama_token_to_str (always returns null)
    ///
    /// # Safety
    /// This is a no-op stub that always returns null. Safe to call but not functional.
    pub unsafe fn llama_token_to_str(
        _ctx: *const llama_context,
        _token: llama_token,
    ) -> *const c_char {
        std::ptr::null()
    }

    /// Stub for llama_free (no-op)
    ///
    /// # Safety
    /// This is a no-op stub. Safe to call but does nothing.
    pub unsafe fn llama_free(_ctx: *mut llama_context) {}

    /// Stub for llama_free_model (no-op)
    ///
    /// # Safety
    /// This is a no-op stub. Safe to call but does nothing.
    pub unsafe fn llama_free_model(_model: *mut llama_model) {}
}

// Re-export FFI functions for explicit import path
#[cfg(feature = "llama-cpp")]
pub use self::{
    llama_decode, llama_free, llama_free_model, llama_model_load, llama_new_context,
    llama_token_to_str, llama_tokenize,
};

// Re-export stubs when feature is disabled
#[cfg(not(feature = "llama-cpp"))]
pub use stub::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_struct_sizes() {
        // Ensure structs are non-zero size
        assert!(std::mem::size_of::<llama_model_params>() > 0);
        assert!(std::mem::size_of::<llama_context_params>() > 0);
    }

    #[test]
    #[cfg(not(feature = "llama-cpp"))]
    fn test_stubs_return_null() {
        // Stubs should return null/error values
        use std::ptr;
        unsafe {
            let model = llama_model_load(
                ptr::null(),
                llama_model_params {
                    n_gpu_layers: 0,
                    main_gpu: 0,
                    use_mmap: false,
                    use_mlock: false,
                },
            );
            assert!(model.is_null());
        }
    }
}
