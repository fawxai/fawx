#![deny(clippy::print_stdout, clippy::print_stderr)]

mod app;
mod credential_reader;
mod fawx_backend;
mod local_auth;
mod markdown_render;
mod render {
    pub mod line_utils;
}
mod wrapping;

pub use app::run_tui;
pub use markdown_render::render_markdown_text;
