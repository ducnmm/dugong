//! Library surface for the Dugong CLI tools.
//!
//! The HTTP/validation logic for the `dugong-login` binary lives in [`login`]
//! so it can be exercised by integration tests against a mock server, keeping
//! the binary itself a thin env-reading wrapper.

pub mod login;
