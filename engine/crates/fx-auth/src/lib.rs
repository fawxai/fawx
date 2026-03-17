//! # fx-auth
//!
//! Authentication and OAuth primitives for Fawx LLM providers.
//! Provides credential/auth method management (`auth`), PKCE OAuth flows
//! plus token parsing helpers (`oauth`), encrypted credential storage
//! (`credential_store`), and GitHub PAT validation (`github`).

pub mod auth;
pub mod credential_store;
pub mod github;
pub mod oauth;
pub mod token_broker;
