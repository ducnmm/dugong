//! Library surface for the Dugong indexer.
//!
//! The binary (`main.rs`) is a thin wrapper over these modules; exposing them
//! as a library lets the integration tests in `tests/` exercise the handlers,
//! cursor manager, and event fetcher directly.

pub mod cursor;
pub mod event_fetcher;
pub mod event_processor;
pub mod handlers;
pub mod indexer;
pub mod types;
