#![deny(clippy::print_stdout, clippy::print_stderr)]

mod app;
pub(crate) mod credential_reader;
mod fawx_backend;
mod markdown_render;
mod render {
    pub mod line_utils;
}
mod wrapping;

pub use app::run_tui;
pub use markdown_render::render_markdown_text;
