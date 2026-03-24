#![deny(clippy::print_stdout, clippy::print_stderr)]

pub const DEFAULT_ENGINE_URL: &str = "http://127.0.0.1:8400";

mod app;
pub(crate) mod credential_reader;
#[cfg(feature = "embedded")]
mod embedded_backend;
#[cfg_attr(not(feature = "embedded"), allow(dead_code))]
pub(crate) mod experiment_panel;
mod fawx_backend;
mod markdown_render;
mod render {
    pub mod line_utils;
}
mod wrapping;

pub use app::{run_tui, RunOptions};
pub use markdown_render::render_markdown_text;
