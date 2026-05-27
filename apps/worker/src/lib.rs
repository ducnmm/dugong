//! Library surface for the Dugong worker (Twitter poller).
//!
//! The binary (`main.rs`) is a thin wrapper; exposing the modules as a library
//! lets the integration tests in `tests/` drive the clients and the
//! tweet→webhook conversion directly.

pub mod backend_client;
pub mod config;
pub mod poller;
pub mod twitter_client;
